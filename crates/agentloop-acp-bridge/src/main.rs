//! ACP agent bridge — connects Zed to the AgentLoop Go server via the
//! Agent Client Protocol (ACP).
//!
//! Zed spawns this binary as an ACP-compatible agent process.  The bridge:
//!   1. Speaks ACP (JSON-RPC 2.0, newline-delimited) over stdin/stdout.
//!   2. Connects to the AgentLoop Go server over a Unix socket on first use.
//!   3. Maps ACP session lifecycle to AgentLoop task lifecycle.
//!   4. Streams AgentLoop events as `session/update` notifications.
//!   5. Forwards HITL approval requests via `session/request_permission`.
//!
//! # ACP Protocol Flow
//!
//! ```text
//! Zed  →  initialize                     →  AgentLoop-ACP-Bridge
//! Zed  ←  initialize (capabilities)      ←
//! Zed  →  session/new {cwd}              →
//! Zed  ←  session/new {sessionId}        ←
//! Zed  →  session/prompt {prompt}        →
//! Zed  ←  session/update (streaming)     ←  (notifications)
//! Zed  ←  session/request_permission     ←  (HITL — optional)
//! Zed  →  session/request_permission res →
//! Zed  ←  session/prompt {stopReason}    ←  (final response)
//! ```

use agentloop_bridge::{AgentEvent, AgentLoopClient, ClientConfig, HITLDecision};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "agentloop-acp-bridge", about = "ACP agent bridge for AgentLoop")]
struct Args {
    /// Path to the AgentLoop Unix socket (overrides config file).
    #[arg(long)]
    socket_path: Option<PathBuf>,

}

// ── ACP protocol types ────────────────────────────────────────────────────────

/// Any incoming JSON-RPC 2.0 message (request, notification, or response).
#[derive(Debug, Deserialize)]
struct AcpIncoming {
    #[allow(dead_code)]
    jsonrpc: String,
    /// Present for requests and responses; absent for notifications.
    #[serde(default)]
    id: Option<Value>,
    /// Present for requests and notifications; absent for responses.
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Value,
    /// Present for responses (success).
    #[serde(default)]
    result: Option<Value>,
    /// Present for responses (error).
    #[serde(default)]
    error: Option<Value>,
}

#[derive(Debug, Serialize)]
struct AcpResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<AcpError>,
}

#[derive(Debug, Serialize)]
struct AcpNotification {
    jsonrpc: &'static str,
    method: &'static str,
    params: Value,
}

#[derive(Debug, Serialize)]
struct AcpRequest {
    jsonrpc: &'static str,
    id: Value,
    method: &'static str,
    params: Value,
}

#[derive(Debug, Serialize)]
struct AcpError {
    code: i32,
    message: String,
}

impl AcpResponse {
    fn ok(id: Value, result: Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self { jsonrpc: "2.0", id, result: None, error: Some(AcpError { code, message: message.into() }) }
    }
}

// ── Session state ─────────────────────────────────────────────────────────────

struct SessionState {
    cwd: String,
    /// AgentLoop session ID set once a task is running.
    agentloop_session_id: Option<String>,
}

// ── Bridge ────────────────────────────────────────────────────────────────────

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

struct Bridge {
    /// Channel for sending serialised JSON-RPC messages to stdout.
    output_tx: mpsc::UnboundedSender<String>,
    /// Outstanding agent→client requests awaiting a response from Zed.
    pending_requests: PendingMap,
    /// Active ACP sessions keyed by ACP session ID.
    sessions: Arc<Mutex<HashMap<String, SessionState>>>,
    /// Shared AgentLoop client (connects lazily).
    agent: Arc<Mutex<AgentLoopClient>>,
    /// Monotonically-increasing request ID counter (for agent→client requests).
    next_req_id: Arc<AtomicU64>,
    /// Monotonically-increasing session counter.
    next_session_num: Arc<AtomicU64>,
}

impl Bridge {
    fn new(output_tx: mpsc::UnboundedSender<String>, config: ClientConfig) -> Self {
        Self {
            output_tx,
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            agent: Arc::new(Mutex::new(AgentLoopClient::new(config))),
            next_req_id: Arc::new(AtomicU64::new(1_000)),
            next_session_num: Arc::new(AtomicU64::new(1)),
        }
    }

    // ── Sending helpers ──────────────────────────────────────────────────────

    fn send_str(&self, s: String) {
        tracing::info!("→ send: {}", s.trim());
        let _ = self.output_tx.send(s);
    }

    fn respond(&self, resp: AcpResponse) {
        if let Ok(json) = serde_json::to_string(&resp) {
            self.send_str(json + "\n");
        }
    }

    fn notify(&self, method: &'static str, params: Value) {
        let notif = AcpNotification { jsonrpc: "2.0", method, params };
        if let Ok(json) = serde_json::to_string(&notif) {
            self.send_str(json + "\n");
        }
    }

    /// Send a JSON-RPC request to the client (Zed) and await its response.
    /// Returns `{"outcome": "cancelled"}` on timeout or channel drop.
    async fn request_client(&self, method: &'static str, params: Value) -> Value {
        let req_id = self.next_req_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel::<Value>();
        self.pending_requests.lock().await.insert(req_id, tx);

        let req = AcpRequest { jsonrpc: "2.0", id: Value::from(req_id), method, params };
        if let Ok(json) = serde_json::to_string(&req) {
            self.send_str(json + "\n");
        }

        match tokio::time::timeout(std::time::Duration::from_secs(120), rx).await {
            Ok(Ok(val)) => val,
            _ => {
                self.pending_requests.lock().await.remove(&req_id);
                json!({"outcome": "cancelled"})
            }
        }
    }

    /// Resolve a pending agent→client request with the client's response.
    async fn resolve_pending(&self, id: u64, result: Value) {
        if let Some(tx) = self.pending_requests.lock().await.remove(&id) {
            let _ = tx.send(result);
        }
    }

    // ── ACP method handlers ──────────────────────────────────────────────────

    /// `initialize` — ACP handshake.
    async fn handle_initialize(&self, id: Value) {
        self.respond(AcpResponse::ok(id, json!({
            "protocolVersion": 1,
            "isAuthenticated": true,
            "agentCapabilities": {
                "loadSession": false,
                "promptCapabilities": {
                    "image": false,
                    "audio": false,
                    "embeddedContext": false
                }
            },
            "agentInfo": {
                "name": "agentloop",
                "title": "AgentLoop",
                "version": env!("CARGO_PKG_VERSION")
            }
        })));
    }

    /// `session/new` — Create a new ACP session.
    async fn handle_session_new(&self, id: Value, params: Value) {
        let cwd = params["cwd"].as_str().unwrap_or("/tmp").to_string();
        let session_num = self.next_session_num.fetch_add(1, Ordering::SeqCst);
        let acp_session_id = format!("acp-sess-{session_num}");

        self.sessions.lock().await.insert(acp_session_id.clone(), SessionState {
            cwd: cwd.to_string(),
            agentloop_session_id: None,
        });

        self.respond(AcpResponse::ok(id, json!({ "sessionId": acp_session_id })));
    }

    /// `session/load` — Load a previous session (best-effort; recreates if missing).
    async fn handle_session_load(&self, id: Value, params: Value) {
        let session_id = params["sessionId"].as_str().unwrap_or("").to_string();
        let cwd = params["cwd"].as_str().unwrap_or("/tmp").to_string();

        let mut sessions = self.sessions.lock().await;
        sessions.entry(session_id).or_insert(SessionState {
            cwd,
            agentloop_session_id: None,
        });
        drop(sessions);

        self.respond(AcpResponse::ok(id, Value::Null));
    }

    /// `session/cancel` — Abort a running AgentLoop task.
    async fn handle_session_cancel(&self, id: Value, params: Value) {
        let session_id = params["sessionId"].as_str().unwrap_or("");
        let al_sid = {
            let sessions = self.sessions.lock().await;
            sessions.get(session_id).and_then(|s| s.agentloop_session_id.clone())
        };

        if let Some(al_sid) = al_sid {
            let mut client = self.agent.lock().await;
            let _ = client.abort_task(&al_sid).await;
        }

        self.respond(AcpResponse::ok(id, Value::Null));
    }

    /// `session/prompt` — Execute a coding task and stream results.
    ///
    /// This is the core ACP handler.  It starts an AgentLoop task, maps every
    /// incoming `AgentEvent` to an ACP `session/update` notification, handles
    /// HITL via `session/request_permission`, and finally responds with a
    /// `stopReason` once the task completes.
    ///
    /// Must be spawned in its own task so the main input loop stays responsive
    /// (needed for reading `session/request_permission` responses from Zed).
    async fn handle_session_prompt(self: Arc<Self>, id: Value, params: Value) {
        let session_id = match params["sessionId"].as_str() {
            Some(s) => s.to_string(),
            None => {
                self.respond(AcpResponse::err(id, -32602, "missing sessionId"));
                return;
            }
        };

        let text = extract_prompt_text(&params["prompt"]);
        if text.is_empty() {
            self.respond(AcpResponse::err(id, -32602, "empty prompt"));
            return;
        }

        let cwd = {
            self.sessions.lock().await
                .get(&session_id)
                .map(|s| s.cwd.clone())
                .unwrap_or_else(|| "/tmp".to_string())
        };

        let user_id = std::env::var("USER").unwrap_or_else(|_| "user".to_string());

        // ── Connect + start task ─────────────────────────────────────────────
        // Create a fresh client per prompt so we get a new event channel each time.
        let config = self.agent.lock().await.config().clone();
        let mut client = AgentLoopClient::new(config);

        if let Err(e) = client.connect().await {
            self.respond(AcpResponse::err(id, -32000, format!("connect failed: {e}")));
            return;
        }

        let al_session_id = match client.start_task(&user_id, &text, Some(cwd), "zed-acp").await {
            Ok(s) => s,
            Err(e) => {
                self.respond(AcpResponse::err(id, -32000, format!("start_task failed: {e}")));
                return;
            }
        };

        let event_rx = client.take_event_receiver();

        // Store AgentLoop session ID so session/cancel can find it.
        {
            let mut sessions = self.sessions.lock().await;
            if let Some(state) = sessions.get_mut(&session_id) {
                state.agentloop_session_id = Some(al_session_id.clone());
            }
        }

        // ── Event streaming ──────────────────────────────────────────────────

        // Track tool call IDs so ToolResult can reference its ToolUse.
        // tool_name → FIFO queue of toolCallIds.
        let mut pending_tool_calls: HashMap<String, VecDeque<String>> = HashMap::new();
        let mut tool_call_counter: u64 = 0;

        let Some(mut rx) = event_rx else {
            self.respond(AcpResponse::ok(id, json!({ "stopReason": "end_turn" })));
            return;
        };

        while let Some(event) = rx.recv().await {
            match event {
                AgentEvent::Text { content, .. } => {
                    self.notify("session/update", json!({
                        "sessionId": session_id,
                        "update": {
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "type": "text", "text": content }
                        }
                    }));
                }

                AgentEvent::ToolUse { tool_name, .. } => {
                    tool_call_counter += 1;
                    let tc_id = format!("tc-{tool_call_counter}");
                    pending_tool_calls
                        .entry(tool_name.clone())
                        .or_default()
                        .push_back(tc_id.clone());

                    self.notify("session/update", json!({
                        "sessionId": session_id,
                        "update": {
                            "sessionUpdate": "tool_call",
                            "toolCallId": tc_id,
                            "title": tool_name,
                            "kind": tool_name_to_kind(&tool_name),
                            "status": "pending"
                        }
                    }));
                }

                AgentEvent::ToolResult { tool_name, output, success, .. } => {
                    let tc_id = pending_tool_calls
                        .get_mut(&tool_name)
                        .and_then(|q| q.pop_front())
                        .unwrap_or_else(|| format!("tc-unknown-{tool_name}"));

                    self.notify("session/update", json!({
                        "sessionId": session_id,
                        "update": {
                            "sessionUpdate": "tool_call_update",
                            "toolCallId": tc_id,
                            "status": if success { "completed" } else { "failed" },
                            "content": [{ "type": "content", "content": { "type": "text", "text": output } }]
                        }
                    }));
                }

                AgentEvent::HITLAutoApproved { tool_name, risk_level, command, .. } => {
                    tracing::info!("HITL auto-approved [risk: {risk_level}] '{tool_name}': {command}");
                    // Inform Zed via a tool_call update so it's visible in the UI.
                    tool_call_counter += 1;
                    let tc_id = format!("tc-auto-{tool_call_counter}");
                    self.notify("session/update", json!({
                        "sessionId": session_id,
                        "update": {
                            "sessionUpdate": "tool_call",
                            "toolCallId": tc_id,
                            "title": format!("Auto-approved [{risk_level}]: {tool_name}"),
                            "kind": tool_name_to_kind(&tool_name),
                            "status": "completed"
                        }
                    }));
                }

                AgentEvent::HITLRequest { session_id: al_sid, request_id, tool_name, details, options, .. } => {
                    tool_call_counter += 1;
                    let tc_id = format!("tc-hitl-{tool_call_counter}");

                    // Map AgentLoop options → ACP permission options.
                    let acp_options: Vec<Value> = options.iter().enumerate().map(|(i, opt)| {
                        let kind = classify_hitl_option(opt);
                        json!({ "optionId": format!("opt-{i}"), "name": opt, "kind": kind })
                    }).collect();

                    let response = self.request_client("session/request_permission", json!({
                        "sessionId": session_id,
                        "toolCall": {
                            "toolCallId": tc_id,
                            "toolName": tool_name,
                            "title": details,
                            "rawInput": details,
                            "kind": tool_name_to_kind(&tool_name)
                        },
                        "options": acp_options
                    })).await;

                    let decision = hitl_decision_from_response(&response, &options);
                    tracing::info!("HITL for '{tool_name}': {decision:?}");

                    let _ = client.respond_hitl(&al_sid, &request_id, decision).await;
                }

                AgentEvent::Done { stats, .. } => {
                    tracing::info!(
                        "Task done — {} tool calls, {}ms (acp_session: {session_id})",
                        stats.tool_calls, stats.duration_ms
                    );
                    break;
                }

                AgentEvent::Error { message, .. } => {
                    tracing::warn!("Task error: {message}");
                    self.respond(AcpResponse::err(id, -32000, message));
                    return;
                }

                _ => {}
            }
        }

        self.respond(AcpResponse::ok(id, json!({ "stopReason": "end_turn" })));
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract plain text from an ACP prompt array (takes the first text block).
fn extract_prompt_text(prompt: &Value) -> String {
    if let Some(arr) = prompt.as_array() {
        for block in arr {
            if block["type"].as_str() == Some("text") {
                if let Some(t) = block["text"].as_str() {
                    return t.to_string();
                }
            }
        }
    }
    prompt.as_str().unwrap_or("").to_string()
}

/// Map AgentLoop tool name to ACP `kind` hint.
fn tool_name_to_kind(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n.contains("read") || n.contains("cat") || n.contains("ls") || n.contains("find") || n.contains("grep") {
        "read"
    } else if n.contains("write") || n.contains("edit") || n.contains("create") || n.contains("mkdir") {
        "edit"
    } else if n.contains("delete") || n.contains("rm") {
        "delete"
    } else if n.contains("move") || n.contains("rename") || n.contains("mv") {
        "move"
    } else if n.contains("search") {
        "search"
    } else if n.contains("bash") || n.contains("sh") || n.contains("exec") || n.contains("run") {
        "execute"
    } else if n.contains("fetch") || n.contains("curl") || n.contains("http") {
        "fetch"
    } else {
        "other"
    }
}

/// Classify an AgentLoop HITL option string as an ACP `kind`.
fn classify_hitl_option(opt: &str) -> &'static str {
    let o = opt.to_lowercase();
    if o.contains("abort") {
        "reject_always"
    } else if o.contains("approve") || o.contains("allow") || o.contains("yes") {
        "allow_once"
    } else {
        "reject_once"
    }
}

/// Map a `session/request_permission` response back to an `HITLDecision`.
fn hitl_decision_from_response(response: &Value, options: &[String]) -> HITLDecision {
    if response["outcome"].as_str() == Some("cancelled") {
        return HITLDecision::Abort;
    }

    if let Some(opt_id) = response["optionId"].as_str() {
        // Parse the numeric suffix from "opt-N".
        let idx: usize = opt_id.strip_prefix("opt-").and_then(|s| s.parse().ok()).unwrap_or(1);
        if let Some(opt) = options.get(idx) {
            let o = opt.to_lowercase();
            if o.contains("approve") || o.contains("allow") || o.contains("yes") {
                return HITLDecision::Approve;
            } else if o.contains("abort") {
                return HITLDecision::Abort;
            }
        }
    }

    HITLDecision::Deny
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // Log to /tmp/agentloop-acp-bridge.log at DEBUG level so runtime errors are
    // visible without cluttering the ACP stdout channel.
    let file_appender = tracing_appender::rolling::never("/tmp", "agentloop-acp-bridge.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_env_filter(
            std::env::var("AGENTLOOP_LOG").unwrap_or_else(|_| "debug".to_string()),
        )
        .init();

    let args = Args::parse();
    let mut config = ClientConfig::load().unwrap_or_default();
    if let Some(socket) = args.socket_path {
        // Expand leading `~/` since Zed spawns the binary directly (no shell).
        config.socket_path = if let Some(s) = socket.to_str() {
            if s.starts_with("~/") {
                let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
                home.join(&s[2..])
            } else {
                socket
            }
        } else {
            socket
        };
    }

    // All output goes through a single unbounded channel → one writer task.
    let (output_tx, mut output_rx) = mpsc::unbounded_channel::<String>();

    let bridge = Arc::new(Bridge::new(output_tx, config));

    // Spawn the output writer task.
    tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(line) = output_rx.recv().await {
            if stdout.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            let _ = stdout.flush().await;
        }
    });

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    tracing::info!("AgentLoop ACP agent bridge ready (v{})", env!("CARGO_PKG_VERSION"));

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break, // EOF or IO error — exit cleanly.
            Ok(_) => {}
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        tracing::info!("← recv: {}", trimmed);

        let msg: AcpIncoming = match serde_json::from_str(trimmed) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("invalid JSON-RPC: {e}");
                continue;
            }
        };

        match msg.method {
            None => {
                // Response from Zed to one of our agent→client requests.
                if let Some(id_val) = msg.id {
                    if let Some(id_num) = id_val.as_u64() {
                        let result = msg.result.or(msg.error).unwrap_or(Value::Null);
                        bridge.resolve_pending(id_num, result).await;
                    }
                }
            }

            Some(method) => {
                // Request or notification from Zed.
                let id = msg.id.clone();

                if id.is_none() {
                    // Notification — no response needed.
                    tracing::debug!("notification: {method}");
                    continue;
                }

                let id = id.unwrap();
                let params = msg.params;
                let b = Arc::clone(&bridge);

                match method.as_str() {
                    "initialize" => b.handle_initialize(id).await,
                    "session/new" => b.handle_session_new(id, params).await,
                    "session/load" => b.handle_session_load(id, params).await,
                    "session/cancel" => b.handle_session_cancel(id, params).await,
                    "session/prompt" => {
                        // Spawned so the main loop stays responsive while the
                        // task runs (needed to read request_permission responses).
                        tokio::spawn(async move {
                            b.handle_session_prompt(id, params).await;
                        });
                    }
                    // Auth is handled server-side; report already-authenticated.
                    "auth/getStatus" => {
                        b.respond(AcpResponse::ok(id, json!({ "authenticated": true })));
                    }
                    "auth/signIn" | "authenticate" => {
                        b.respond(AcpResponse::ok(id, json!({ "authenticated": true })));
                    }
                    other => {
                        b.respond(AcpResponse::err(id, -32601, format!("method not found: {other}")));
                    }
                }
            }
        }
    }
}
