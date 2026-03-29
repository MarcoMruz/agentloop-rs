//! AgentLoop Bridge - Client Library for AgentLoop Server
//!
//! This crate provides a client library for communicating with the AgentLoop server.
//! It implements the JSON-RPC 2.0 protocol over Unix domain socket for accessing
//! memory management, HITL approval, session management, and agent execution.
//!
//! # Features
//!
//! - `zed-acp`: Enable Zed Adaptive Code Provider (ACP) integration
//!
//! # Example
//!
//! ```rust,no_run
//! use agentloop_bridge::{AgentLoopClient, ClientConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = ClientConfig::default();
//!     let mut client = AgentLoopClient::new(config);
//!
//!     // Connect to AgentLoop server
//!     client.connect().await?;
//!
//!     // Start a task
//!     let session_id = client.start_task("marco", "Fix the bugs in this code", None, "zed").await?;
//!
//!     // Wait for completion
//!     let result = client.wait_for_completion(&session_id).await?;
//!
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]
#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::net::UnixStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};


/// Configuration for the AgentLoop client
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Path to AgentLoop server Unix socket
    pub socket_path: PathBuf,
    /// Maximum request timeout
    pub request_timeout: Duration,
    /// Connection retry attempts
    pub max_retries: u32,
    /// Retry delay
    pub retry_delay: Duration,
    /// Event buffer size
    pub event_buffer_size: usize,
}

/// YAML-deserialisable subset of the config file (only the fields the client cares about)
#[derive(serde::Deserialize, Default)]
struct FileConfig {
    server: Option<FileServerConfig>,
}

#[derive(serde::Deserialize)]
struct FileServerConfig {
    socket_path: Option<PathBuf>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));

        Self {
            socket_path: home.join(".local/share/agentloop/agentloop.sock"),
            request_timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_delay: Duration::from_millis(1000),
            event_buffer_size: 1000,
        }
    }
}

impl ClientConfig {
    /// Load from the default config file (`~/.config/agentloop/agentloop.yaml`),
    /// falling back to defaults for any missing values.
    pub fn load() -> Result<Self> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        Self::load_from_path(home.join(".config/agentloop/agentloop.yaml"))
    }

    /// Load from a specific config file path, falling back to defaults for any missing values.
    pub fn load_from_path(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let mut cfg = Self::default();

        if !path.exists() {
            return Ok(cfg);
        }

        let file: FileConfig = config::Config::builder()
            .add_source(config::File::from(path))
            .build()
            .map_err(|e| BridgeError::Config { message: e.to_string() })?
            .try_deserialize()
            .map_err(|e| BridgeError::Config { message: e.to_string() })?;

        if let Some(server) = file.server {
            if let Some(socket_path) = server.socket_path {
                // Expand leading `~/`
                cfg.socket_path = if socket_path.starts_with("~") {
                    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
                    let stripped = socket_path.strip_prefix("~").unwrap_or(&socket_path);
                    home.join(stripped.strip_prefix("/").unwrap_or(stripped))
                } else {
                    socket_path
                };
            }
        }

        Ok(cfg)
    }
}

/// Client connection state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientState {
    /// Disconnected
    Disconnected,
    /// Connecting
    Connecting,
    /// Connected and ready
    Connected,
    /// Reconnecting after disconnect
    Reconnecting,
}

/// Events from AgentLoop server
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Connection state changed
    StateChanged(ClientState),
    /// Text output from the agent
    Text {
        /// Session ID
        session_id: String,
        /// Text content
        content: String,
    },
    /// Tool execution started
    ToolUse {
        /// Session ID
        session_id: String,
        /// Tool name
        tool_name: String,
        /// Tool arguments
        input: serde_json::Value,
    },
    /// Tool execution completed
    ToolResult {
        /// Session ID
        session_id: String,
        /// Tool name
        tool_name: String,
        /// Tool output
        output: String,
        /// Whether tool succeeded
        success: bool,
    },
    /// HITL approval request
    HITLRequest {
        /// Session ID
        session_id: String,
        /// Request ID for correlation
        request_id: String,
        /// Tool name requiring approval
        tool_name: String,
        /// Human-readable details
        details: String,
        /// Available options
        options: Vec<String>,
        /// Raw command / file path / URL (optional)
        command: Option<String>,
        /// Working directory at time of request (optional)
        work_dir: Option<String>,
        /// Security rule name that fired (optional)
        rule: Option<String>,
        /// Sub-method within the rule (optional)
        method: Option<String>,
        /// Tool category hint from server (optional)
        tool_category: Option<String>,
        /// Specific file/dir path for file tools (optional)
        file_path: Option<String>,
        /// Risk level: "low" | "medium" | "high" (optional)
        risk_level: Option<String>,
        /// One-line human explanation of why approval is needed (optional)
        reason: Option<String>,
    },
    /// HITL request was automatically approved by the server
    /// (risk level is "low" or "medium" and AutoApproveNonHigh is enabled)
    HITLAutoApproved {
        /// Session ID
        session_id: String,
        /// Request ID for correlation
        request_id: String,
        /// Tool name that was auto-approved
        tool_name: String,
        /// Risk level ("low" or "medium")
        risk_level: String,
        /// Raw command / file path that was approved
        command: String,
        /// Security rule name
        rule: String,
    },
    /// Agent completed task
    Done {
        /// Session ID
        session_id: String,
        /// Final output
        output: String,
        /// Task statistics
        stats: TaskStats,
    },
    /// Error occurred
    Error {
        /// Session ID
        session_id: String,
        /// Error message
        message: String,
    },
    /// Session was saved to vault
    SessionSaved {
        /// Session ID
        session_id: String,
    },
}

/// Task execution statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStats {
    /// Total duration
    pub duration_ms: u64,
    /// Number of tool calls
    pub tool_calls: u32,
    /// Tokens used (if available)
    pub tokens_used: Option<u32>,
    /// Number of HITL requests
    pub hitl_requests: u32,
}

/// HITL decision response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HITLDecision {
    /// Approve the action
    Approve,
    /// Deny the action
    Deny,
    /// Abort the entire task
    Abort,
}

/// JSON-RPC 2.0 request
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
    pub id: u64,
}

/// JSON-RPC 2.0 response
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcError>,
    pub id: Option<u64>,
}

/// JSON-RPC 2.0 error
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 notification
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
}

/// Task start parameters
#[derive(Debug, Serialize)]
pub struct TaskStartParams {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub text: String,
    #[serde(rename = "workDir", skip_serializing_if = "Option::is_none")]
    pub work_dir: Option<String>,
    pub source: String,
}

/// Task start response
#[derive(Debug, Deserialize)]
pub struct TaskStartResponse {
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

/// Pending request for correlation
#[derive(Debug)]
struct PendingRequest {
    tx: tokio::sync::oneshot::Sender<Result<serde_json::Value>>,
    timeout: tokio::time::Instant,
}

/// Main AgentLoop client
pub struct AgentLoopClient {
    config: ClientConfig,
    state: ClientState,
    stream_writer: Option<Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>>,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    event_rx: Option<mpsc::UnboundedReceiver<AgentEvent>>,
    pending_requests: Arc<tokio::sync::Mutex<HashMap<u64, PendingRequest>>>,
    next_id: Arc<AtomicU64>,
    active_sessions: Arc<tokio::sync::RwLock<HashMap<String, TaskStats>>>,
    _read_task: Option<tokio::task::JoinHandle<()>>,
}

impl AgentLoopClient {
    /// Create a new client instance
    pub fn new(config: ClientConfig) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        Self {
            config,
            state: ClientState::Disconnected,
            stream_writer: None,
            event_tx,
            event_rx: Some(event_rx),
            pending_requests: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
            active_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            _read_task: None,
        }
    }

    /// Get event receiver (can only be called once)
    pub fn take_event_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<AgentEvent>> {
        self.event_rx.take()
    }

    /// Get the client configuration.
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Get current connection state
    pub fn state(&self) -> ClientState {
        self.state.clone()
    }

    /// Connect to the AgentLoop server
    pub async fn connect(&mut self) -> Result<()> {
        self.state = ClientState::Connecting;
        self.emit_state_change().await;

        let stream = UnixStream::connect(&self.config.socket_path).await?;
        let (reader, writer) = stream.into_split();

        // Start read task for handling server messages
        let pending_requests = Arc::clone(&self.pending_requests);
        let event_tx = self.event_tx.clone();
        let active_sessions = Arc::clone(&self.active_sessions);

        let read_task = tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if let Err(e) = Self::handle_server_message(&line, &pending_requests, &event_tx, &active_sessions).await {
                            tracing::warn!("Error handling server message: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error reading from server: {}", e);
                        break;
                    }
                }
            }
        });

        // Store writer wrapped in a mutex for thread-safe access
        self.stream_writer = Some(Arc::new(tokio::sync::Mutex::new(writer)));
        self._read_task = Some(read_task);
        self.state = ClientState::Connected;
        self.emit_state_change().await;

        Ok(())
    }

    /// Start a new task
    pub async fn start_task(
        &mut self,
        user_id: impl Into<String>,
        text: impl Into<String>,
        work_dir: Option<String>,
        source: impl Into<String>
    ) -> Result<String> {
        let params = TaskStartParams {
            user_id: user_id.into(),
            text: text.into(),
            work_dir,
            source: source.into(),
        };

        let response = self.send_request("task.start", serde_json::to_value(params)?).await?;
        let start_response: TaskStartResponse = serde_json::from_value(response)?;

        // Track session
        let mut sessions = self.active_sessions.write().await;
        sessions.insert(start_response.session_id.clone(), TaskStats {
            duration_ms: 0,
            tool_calls: 0,
            tokens_used: None,
            hitl_requests: 0,
        });

        Ok(start_response.session_id)
    }

    /// Steer an active task
    pub async fn steer_task(&mut self, session_id: impl Into<String>, text: impl Into<String>) -> Result<()> {
        let params = serde_json::json!({
            "sessionId": session_id.into(),
            "text": text.into()
        });

        self.send_request("task.steer", params).await?;
        Ok(())
    }

    /// Abort a task
    pub async fn abort_task(&mut self, session_id: impl Into<String>) -> Result<()> {
        let params = serde_json::json!({
            "sessionId": session_id.into()
        });

        self.send_request("task.abort", params).await?;
        Ok(())
    }

    /// Respond to a HITL request
    pub async fn respond_hitl(
        &mut self,
        session_id: impl Into<String>,
        request_id: impl Into<String>,
        decision: HITLDecision
    ) -> Result<()> {
        let params = serde_json::json!({
            "sessionId": session_id.into(),
            "requestId": request_id.into(),
            "decision": decision
        });

        self.send_request("hitl.respond", params).await?;
        Ok(())
    }

    /// Check server health
    pub async fn health_check(&mut self) -> Result<serde_json::Value> {
        self.send_request("health.check", serde_json::json!({})).await
    }

    /// Wait for a specific task to complete
    pub async fn wait_for_completion(&mut self, session_id: &str) -> Result<TaskStats> {
        let mut event_rx = self.event_rx.take().ok_or_else(|| {
            BridgeError::Config { message: "Event receiver already taken".to_string() }
        })?;

        while let Some(event) = event_rx.recv().await {
            match event {
                AgentEvent::Done { session_id: done_session_id, stats, .. } => {
                    if done_session_id == session_id {
                        // Remove from active sessions
                        self.active_sessions.write().await.remove(session_id);
                        self.event_rx = Some(event_rx);
                        return Ok(stats);
                    }
                }
                AgentEvent::Error { session_id: error_session_id, message } => {
                    if error_session_id == session_id {
                        self.active_sessions.write().await.remove(session_id);
                        self.event_rx = Some(event_rx);
                        return Err(BridgeError::Process { message });
                    }
                }
                _ => {} // Continue waiting
            }
        }

        Err(BridgeError::Process { message: "Event stream ended before completion".to_string() })
    }

    /// Disconnect from server
    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(task) = self._read_task.take() {
            task.abort();
        }

        self.stream_writer = None;
        self.state = ClientState::Disconnected;
        self.emit_state_change().await;

        Ok(())
    }

    /// Send a JSON-RPC request and wait for response
    async fn send_request(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let stream_writer = self.stream_writer.as_ref().ok_or_else(|| {
            BridgeError::Config { message: "Not connected".to_string() }
        })?.clone();

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id,
        };

        // Prepare response channel
        let (tx, rx) = tokio::sync::oneshot::channel();
        let pending_request = PendingRequest {
            tx,
            timeout: tokio::time::Instant::now() + self.config.request_timeout,
        };

        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(id, pending_request);
        }

        // Send request
        let request_line = serde_json::to_string(&request)? + "\n";
        {
            let mut writer = stream_writer.lock().await;
            writer.write_all(request_line.as_bytes()).await?;
        }

        // Wait for response or timeout
        tokio::time::timeout(self.config.request_timeout, rx).await
            .map_err(|_| BridgeError::Timeout { message: format!("Request {} timed out", method) })?
            .map_err(|_| BridgeError::Process { message: "Response channel closed".to_string() })?
    }

    /// Handle incoming message from server
    async fn handle_server_message(
        line: &str,
        pending_requests: &Arc<tokio::sync::Mutex<HashMap<u64, PendingRequest>>>,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        active_sessions: &Arc<tokio::sync::RwLock<HashMap<String, TaskStats>>>,
    ) -> Result<()> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(());
        }

        let json: serde_json::Value = serde_json::from_str(line)?;

        // Check if it's a response (has id) or notification (no id)
        if let Some(id) = json.get("id").and_then(|v| v.as_u64()) {
            // Handle response
            let response: JsonRpcResponse = serde_json::from_value(json)?;

            let mut pending = pending_requests.lock().await;
            if let Some(pending_request) = pending.remove(&id) {
                let result = if let Some(error) = response.error {
                    Err(BridgeError::Process { message: error.message })
                } else {
                    Ok(response.result.unwrap_or(serde_json::Value::Null))
                };
                let _ = pending_request.tx.send(result);
            }
        } else {
            // Handle notification (event)
            let notification: JsonRpcNotification = serde_json::from_value(json)?;
            Self::handle_event(notification, event_tx, active_sessions).await?;
        }

        Ok(())
    }

    /// Handle server event notification
    async fn handle_event(
        notification: JsonRpcNotification,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        active_sessions: &Arc<tokio::sync::RwLock<HashMap<String, TaskStats>>>,
    ) -> Result<()> {
        let event = match notification.method.as_str() {
            "event.text" => {
                let session_id = notification.params["sessionId"].as_str().unwrap_or("").to_string();
                let content = notification.params["content"].as_str().unwrap_or("").to_string();

                AgentEvent::Text { session_id, content }
            }
            "event.tool_use" => {
                let session_id = notification.params["sessionId"].as_str().unwrap_or("").to_string();
                let tool_name = notification.params["toolName"].as_str().unwrap_or("").to_string();
                let input = notification.params["input"].clone();

                // Update stats
                if let Some(ref mut sessions) = active_sessions.write().await.get_mut(&session_id) {
                    sessions.tool_calls += 1;
                }

                AgentEvent::ToolUse { session_id, tool_name, input }
            }
            "event.tool_result" => {
                let session_id = notification.params["sessionId"].as_str().unwrap_or("").to_string();
                let tool_name = notification.params["toolName"].as_str().unwrap_or("").to_string();
                let output = notification.params["output"].as_str().unwrap_or("").to_string();
                let success = notification.params["success"].as_bool().unwrap_or(false);

                AgentEvent::ToolResult { session_id, tool_name, output, success }
            }
            "event.hitl_request" => {
                let session_id = notification.params["sessionId"].as_str().unwrap_or("").to_string();
                let request_id = notification.params["requestId"].as_str().unwrap_or("").to_string();
                let tool_name = notification.params["toolName"].as_str().unwrap_or("").to_string();
                let details = notification.params["details"].as_str().unwrap_or("").to_string();
                let options = notification.params["options"]
                    .as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();

                // Enriched optional fields
                let command = notification.params["command"].as_str().map(String::from);
                let work_dir = notification.params["workDir"].as_str().map(String::from);
                let rule = notification.params["rule"].as_str().map(String::from);
                let method = notification.params["method"].as_str().map(String::from);
                let tool_category = notification.params["toolCategory"].as_str().map(String::from);
                let file_path = notification.params["filePath"].as_str().map(String::from);
                let risk_level = notification.params["riskLevel"].as_str().map(String::from);
                let reason = notification.params["reason"].as_str().map(String::from);

                // Update stats
                if let Some(ref mut sessions) = active_sessions.write().await.get_mut(&session_id) {
                    sessions.hitl_requests += 1;
                }

                AgentEvent::HITLRequest {
                    session_id, request_id, tool_name, details, options,
                    command, work_dir, rule, method, tool_category, file_path,
                    risk_level, reason,
                }
            }
            "event.hitl_auto_approved" => {
                let session_id = notification.params["sessionId"].as_str().unwrap_or("").to_string();
                let request_id = notification.params["requestId"].as_str().unwrap_or("").to_string();
                let tool_name = notification.params["toolName"].as_str().unwrap_or("").to_string();
                let risk_level = notification.params["riskLevel"].as_str().unwrap_or("").to_string();
                let command = notification.params["command"].as_str().unwrap_or("").to_string();
                let rule = notification.params["rule"].as_str().unwrap_or("").to_string();

                AgentEvent::HITLAutoApproved { session_id, request_id, tool_name, risk_level, command, rule }
            }
            "event.done" => {
                let session_id = notification.params["sessionId"].as_str().unwrap_or("").to_string();
                let output = notification.params["output"].as_str().unwrap_or("").to_string();
                let stats = serde_json::from_value(notification.params["stats"].clone())
                    .unwrap_or_else(|_| TaskStats {
                        duration_ms: 0,
                        tool_calls: 0,
                        tokens_used: None,
                        hitl_requests: 0,
                    });

                AgentEvent::Done { session_id, output, stats }
            }
            "event.error" => {
                let session_id = notification.params["sessionId"].as_str().unwrap_or("").to_string();
                let message = notification.params["message"].as_str().unwrap_or("").to_string();

                AgentEvent::Error { session_id, message }
            }
            "event.session_saved" => {
                let session_id = notification.params["sessionId"].as_str().unwrap_or("").to_string();

                AgentEvent::SessionSaved { session_id }
            }
            _ => return Ok(()), // Unknown event, ignore
        };

        let _ = event_tx.send(event);
        Ok(())
    }

    /// Emit state change event
    async fn emit_state_change(&self) {
        let _ = self.event_tx.send(AgentEvent::StateChanged(self.state.clone()));
    }

    /// Check if client is connected
    pub fn is_connected(&self) -> bool {
        matches!(self.state, ClientState::Connected)
    }

    /// Get list of active session IDs
    pub async fn active_session_ids(&self) -> Vec<String> {
        self.active_sessions.read().await.keys().cloned().collect()
    }
}

/// Bridge error types
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Process error
    #[error("Process error: {message}")]
    Process { message: String },
    /// Timeout error
    #[error("Timeout: {message}")]
    Timeout { message: String },
    /// Invalid state error
    #[error("Invalid state: expected {expected:?}, got {actual:?}")]
    InvalidState { expected: ClientState, actual: ClientState },
    /// Configuration error
    #[error("Configuration error: {message}")]
    Config { message: String },
}

/// Result type for bridge operations
pub type Result<T> = std::result::Result<T, BridgeError>;

#[cfg(feature = "zed-acp")]
/// Zed ACP integration module
pub mod zed_acp {
    //! Integration with Zed Adaptive Code Provider infrastructure
    //!
    //! This module provides adapters to use the AgentLoop client within Zed's ACP system.

    use super::*;
    use std::path::{Path, PathBuf};

    /// A single file's content, for injecting into task prompts as context.
    #[derive(Debug, Clone)]
    pub struct FileContext {
        /// Path to the file (used for display and language detection).
        pub path: String,
        /// Full text content of the file.
        pub content: String,
    }

    impl FileContext {
        /// Read a single file from disk.
        pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
            let path = path.as_ref();
            let content = std::fs::read_to_string(path)
                .map_err(BridgeError::Io)?;
            Ok(Self {
                path: path.to_string_lossy().into_owned(),
                content,
            })
        }

        /// Collect files from a directory according to `options`.
        pub fn from_folder(path: impl AsRef<Path>, options: &FolderContextOptions) -> Result<Vec<Self>> {
            let mut results = Vec::new();
            collect_files(path.as_ref(), options, &mut results, 0)?;
            results.sort_by(|a, b| a.path.cmp(&b.path));
            Ok(results)
        }

        fn language_hint(&self) -> &str {
            Path::new(&self.path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
        }

        /// Format as a fenced markdown code block.
        pub fn as_markdown_block(&self) -> String {
            format!(
                "### `{}`\n```{}\n{}\n```\n",
                self.path,
                self.language_hint(),
                self.content.trim_end()
            )
        }
    }

    fn collect_files(
        dir: &Path,
        options: &FolderContextOptions,
        results: &mut Vec<FileContext>,
        depth: usize,
    ) -> Result<()> {
        if results.len() >= options.max_files {
            return Ok(());
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };
        let default_exts = ["rs", "toml", "go", "ts", "tsx", "js", "jsx", "py", "md", "yaml", "yml", "json"];
        let mut subdirs: Vec<PathBuf> = Vec::new();

        for entry in entries.flatten() {
            if results.len() >= options.max_files {
                break;
            }
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if options.recursive && depth < options.max_depth && !options.ignore_dirs.iter().any(|d| d == name) {
                    subdirs.push(path);
                }
            } else if path.is_file() {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let allowed = match &options.extensions {
                    Some(exts) => exts.iter().any(|e| e == ext),
                    None => default_exts.contains(&ext),
                };
                if !allowed {
                    continue;
                }
                let size = path.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
                if size > options.max_file_size_bytes as u64 {
                    continue;
                }
                if let Ok(ctx) = FileContext::from_file(&path) {
                    results.push(ctx);
                }
            }
        }
        for subdir in subdirs {
            if results.len() >= options.max_files {
                break;
            }
            collect_files(&subdir, options, results, depth + 1)?;
        }
        Ok(())
    }

    /// Options controlling which files are collected from a folder.
    pub struct FolderContextOptions {
        /// Extensions to include (e.g. `["rs", "toml"]`). `None` uses a built-in default set.
        pub extensions: Option<Vec<String>>,
        /// Maximum number of files to collect. Default: 20.
        pub max_files: usize,
        /// Maximum file size in bytes; larger files are skipped. Default: 100 KiB.
        pub max_file_size_bytes: usize,
        /// Recurse into subdirectories. Default: true.
        pub recursive: bool,
        /// Maximum recursion depth. Default: 5.
        pub max_depth: usize,
        /// Directory names to skip entirely. Default: `["target", ".git", "node_modules", ".cargo", "dist", "build"]`.
        pub ignore_dirs: Vec<String>,
    }

    impl Default for FolderContextOptions {
        fn default() -> Self {
            Self {
                extensions: None,
                max_files: 20,
                max_file_size_bytes: 100 * 1024,
                recursive: true,
                max_depth: 5,
                ignore_dirs: vec![
                    "target".into(),
                    ".git".into(),
                    "node_modules".into(),
                    ".cargo".into(),
                    "dist".into(),
                    "build".into(),
                ],
            }
        }
    }

    /// Zed ACP adapter for AgentLoop client
    pub struct ZedACPAdapter {
        client: AgentLoopClient,
        current_user: String,
    }

    impl ZedACPAdapter {
        /// Create new ACP adapter
        pub fn new(config: ClientConfig, user_id: String) -> Self {
            Self {
                client: AgentLoopClient::new(config),
                current_user: user_id,
            }
        }

        /// Get the underlying client
        pub fn client(&self) -> &AgentLoopClient {
            &self.client
        }

        /// Get mutable access to the underlying client
        pub fn client_mut(&mut self) -> &mut AgentLoopClient {
            &mut self.client
        }

        /// Current user ID
        pub fn user_id(&self) -> &str {
            &self.current_user
        }

        /// Start a task with context-enriched prompt, returning the session ID.
        /// The caller is responsible for consuming events via `client_mut().take_event_receiver()`.
        pub async fn start_task_with_context(
            &mut self,
            prompt: &str,
            workspace_path: Option<&str>,
        ) -> Result<String> {
            if !self.client.is_connected() {
                self.client.connect().await?;
            }

            let enhanced_prompt = self.build_context_prompt(prompt, workspace_path).await?;

            self.client
                .start_task(&self.current_user, &enhanced_prompt, workspace_path.map(String::from), "zed-acp")
                .await
        }

        /// Start a task with explicit file contexts embedded in the prompt.
        ///
        /// Each `FileContext` in `files` has its full content included as a fenced code block.
        /// Use [`FileContext::from_file`] for individual files or [`FileContext::from_folder`] for
        /// entire directories.
        ///
        /// # Example
        /// ```no_run
        /// # use agentloop_bridge::zed_acp::{ZedACPAdapter, FileContext, FolderContextOptions};
        /// # use agentloop_bridge::ClientConfig;
        /// # async fn example() -> agentloop_bridge::Result<()> {
        /// let config = ClientConfig::default();
        /// let mut adapter = ZedACPAdapter::new(config, "marco".to_string());
        ///
        /// // Single file
        /// let file = FileContext::from_file("src/main.rs")?;
        ///
        /// // Entire folder (Rust files only, max 10 files)
        /// let folder_opts = FolderContextOptions {
        ///     extensions: Some(vec!["rs".into()]),
        ///     max_files: 10,
        ///     ..Default::default()
        /// };
        /// let folder_files = FileContext::from_folder("src/", &folder_opts)?;
        ///
        /// let mut all_files = vec![file];
        /// all_files.extend(folder_files);
        ///
        /// let session_id = adapter
        ///     .start_task_with_files("Fix all compiler warnings", Some("/my/project"), &all_files)
        ///     .await?;
        /// # Ok(())
        /// # }
        /// ```
        pub async fn start_task_with_files(
            &mut self,
            prompt: &str,
            work_dir: Option<&str>,
            files: &[FileContext],
        ) -> Result<String> {
            if !self.client.is_connected() {
                self.client.connect().await?;
            }
            let enhanced_prompt = Self::format_task_with_files(prompt, files);
            self.client
                .start_task(&self.current_user, &enhanced_prompt, work_dir.map(String::from), "zed-acp")
                .await
        }

        /// Compose a prompt that embeds file contents as fenced code blocks.
        ///
        /// Returns the raw string — useful when you want to inspect or log the prompt before
        /// sending it, or when building prompts outside of an async context.
        pub fn format_task_with_files(prompt: &str, files: &[FileContext]) -> String {
            if files.is_empty() {
                return prompt.to_string();
            }
            let mut out = String::from("## Context Files\n\n");
            for file in files {
                out.push_str(&file.as_markdown_block());
                out.push('\n');
            }
            out.push_str(&format!("## Task\n\n{}", prompt));
            out
        }

        /// Build an enriched prompt with workspace context (files, git status).
        pub async fn build_context_prompt(&self, prompt: &str, workspace_path: Option<&str>) -> Result<String> {
            let mut context = String::new();

            if let Some(workspace) = workspace_path {
                context.push_str(&format!("Workspace: {}\n", workspace));

                let files = self.get_relevant_files(workspace).await?;
                for file in &files {
                    context.push_str(&format!("File: {}\n", file));
                }
            }

            if let Some(current_file) = self.get_current_file().await? {
                context.push_str(&format!("\nCurrent file: {}\n", current_file));
            }

            if let Some(git_status) = self.get_git_status(workspace_path).await? {
                context.push_str(&format!("\nGit status:\n{}\n", git_status));
            }

            Ok(format!("{}\nUser request: {}", context, prompt))
        }

        /// Scan workspace root for relevant source files (non-recursive, top-level only).
        async fn get_relevant_files(&self, workspace_path: &str) -> Result<Vec<String>> {
            let mut files = Vec::new();
            if let Ok(entries) = std::fs::read_dir(workspace_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(ext) = path.extension() {
                            if matches!(ext.to_str(), Some("rs" | "toml" | "go" | "ts" | "js" | "py" | "md")) {
                                if let Some(p) = path.to_str() {
                                    files.push(p.to_string());
                                }
                            }
                        }
                    }
                }
            }
            Ok(files)
        }

        /// Returns the currently focused file (populated by Zed context in Phase 2).
        async fn get_current_file(&self) -> Result<Option<String>> {
            Ok(None)
        }

        /// Run `git status --short` in the workspace directory.
        async fn get_git_status(&self, workspace_path: Option<&str>) -> Result<Option<String>> {
            let mut cmd = tokio::process::Command::new("git");
            cmd.args(["status", "--short"]);
            if let Some(path) = workspace_path {
                cmd.current_dir(path);
            }
            match cmd.output().await {
                Ok(output) if output.status.success() => {
                    let status = String::from_utf8_lossy(&output.stdout).to_string();
                    if status.trim().is_empty() {
                        Ok(None)
                    } else {
                        Ok(Some(status))
                    }
                }
                _ => Ok(None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_default() {
        let config = ClientConfig::default();
        assert!(config.socket_path.to_string_lossy().contains("agentloop.sock"));
        assert_eq!(config.request_timeout, Duration::from_secs(30));
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_client_creation() {
        let config = ClientConfig::default();
        let client = AgentLoopClient::new(config);
        assert_eq!(client.state(), ClientState::Disconnected);
        assert!(!client.is_connected());
    }

    #[test]
    fn test_json_rpc_request_serialization() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "task.start".to_string(),
            params: serde_json::json!({"userId": "test", "text": "hello", "source": "zed"}),
            id: 1,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"task.start\""));
        assert!(json.contains("\"id\":1"));
    }

    #[test]
    fn test_task_start_params_serialization() {
        let params = TaskStartParams {
            user_id: "marco".to_string(),
            text: "Fix the tests".to_string(),
            work_dir: Some("/home/marco/project".to_string()),
            source: "zed".to_string(),
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"userId\":\"marco\""));
        assert!(json.contains("\"workDir\":\"/home/marco/project\""));
        assert!(json.contains("\"source\":\"zed\""));
    }

    #[test]
    fn test_hitl_decision_serialization() {
        let decision = HITLDecision::Approve;
        let json = serde_json::to_string(&decision).unwrap();
        assert_eq!(json, "\"approve\"");

        let decision = HITLDecision::Deny;
        let json = serde_json::to_string(&decision).unwrap();
        assert_eq!(json, "\"deny\"");

        let decision = HITLDecision::Abort;
        let json = serde_json::to_string(&decision).unwrap();
        assert_eq!(json, "\"abort\"");
    }

    #[test]
    fn test_task_stats() {
        let stats = TaskStats {
            duration_ms: 5000,
            tool_calls: 3,
            tokens_used: Some(1234),
            hitl_requests: 1,
        };

        let json = serde_json::to_string(&stats).unwrap();
        let parsed: TaskStats = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.duration_ms, 5000);
        assert_eq!(parsed.tool_calls, 3);
        assert_eq!(parsed.tokens_used, Some(1234));
        assert_eq!(parsed.hitl_requests, 1);
    }

    #[cfg(feature = "zed-acp")]
    #[test]
    fn test_zed_acp_adapter_creation() {
        let config = ClientConfig::default();
        let adapter = crate::zed_acp::ZedACPAdapter::new(config, "test_user".to_string());
        assert_eq!(adapter.client().state(), ClientState::Disconnected);
    }

    #[cfg(feature = "zed-acp")]
    #[test]
    fn test_file_context_as_markdown_block() {
        use crate::zed_acp::FileContext;
        let ctx = FileContext {
            path: "src/main.rs".to_string(),
            content: "fn main() {}".to_string(),
        };
        let block = ctx.as_markdown_block();
        assert!(block.contains("### `src/main.rs`"));
        assert!(block.contains("```rs"));
        assert!(block.contains("fn main() {}"));
    }

    #[cfg(feature = "zed-acp")]
    #[test]
    fn test_format_task_with_files_empty() {
        use crate::zed_acp::ZedACPAdapter;
        let result = ZedACPAdapter::format_task_with_files("fix bug", &[]);
        assert_eq!(result, "fix bug");
    }

    #[cfg(feature = "zed-acp")]
    #[test]
    fn test_format_task_with_files_with_content() {
        use crate::zed_acp::{FileContext, ZedACPAdapter};
        let files = vec![FileContext {
            path: "src/lib.rs".to_string(),
            content: "pub fn add(a: i32, b: i32) -> i32 { a + b }".to_string(),
        }];
        let result = ZedACPAdapter::format_task_with_files("add tests", &files);
        assert!(result.contains("## Context Files"));
        assert!(result.contains("src/lib.rs"));
        assert!(result.contains("## Task"));
        assert!(result.contains("add tests"));
    }

    #[cfg(feature = "zed-acp")]
    #[test]
    fn test_folder_context_options_default() {
        use crate::zed_acp::FolderContextOptions;
        let opts = FolderContextOptions::default();
        assert_eq!(opts.max_files, 20);
        assert_eq!(opts.max_file_size_bytes, 100 * 1024);
        assert!(opts.recursive);
        assert_eq!(opts.max_depth, 5);
        assert!(opts.ignore_dirs.contains(&"target".to_string()));
        assert!(opts.ignore_dirs.contains(&".git".to_string()));
    }

    #[cfg(feature = "zed-acp")]
    #[test]
    fn test_file_context_from_file() {
        use crate::zed_acp::FileContext;
        // Read this very test file as a sanity check
        let path = file!(); // e.g. "crates/agentloop-bridge/src/lib.rs"
        if let Ok(ctx) = FileContext::from_file(path) {
            assert!(!ctx.content.is_empty());
            assert!(ctx.path.contains("lib.rs"));
        }
    }
}
