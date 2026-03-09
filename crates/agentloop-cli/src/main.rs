//! AgentLoop CLI Client
//! 
//! Connects to the AgentLoop server via UNIX socket and provides a command-line interface.

use agentloop_bridge::ClientConfig;
use anyhow::{Result, Context};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tracing::{info, warn, error};

/// AgentLoop CLI client
#[derive(Parser)]
#[command(name = "agentloop")]
#[command(about = "AgentLoop CLI client - interact with coding agents")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    
    /// Task description (if no subcommand provided)
    #[arg(value_name = "TASK")]
    task: Option<String>,
    
    /// Working directory
    #[arg(short, long)]
    work_dir: Option<String>,
    
    /// User ID
    #[arg(short, long, default_value = "default")]
    user: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a new agent task
    Task {
        /// Task description
        description: String,
        /// Working directory
        #[arg(short, long)]
        work_dir: Option<String>,
    },
    /// List active sessions
    Sessions {
        /// Show sessions for specific user
        #[arg(short, long)]
        user: Option<String>,
    },
    /// Check server health
    Health,
    /// Abort a running session
    Abort {
        /// Session ID to abort
        session_id: String,
    },
    /// Get memory context for user
    Memory {
        /// User ID
        #[arg(short, long, default_value = "default")]
        user: String,
    },
}

/// JSON-RPC request
#[derive(serde::Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Value,
    id: u64,
}

/// JSON-RPC response
#[derive(serde::Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    result: Option<Value>,
    error: Option<JsonRpcError>,
    id: Option<Value>,
}

/// JSON-RPC error
#[derive(serde::Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    data: Option<Value>,
}

/// JSON-RPC notification
#[derive(serde::Deserialize)]
struct JsonRpcNotification {
    jsonrpc: String,
    method: String,
    params: Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing for CLI
    tracing_subscriber::fmt()
        .with_env_filter("agentloop=info")
        .init();

    let cli = Cli::parse();

    // Load config to get socket path
    let config = ClientConfig::load().context("Failed to load configuration")?;

    // Connect to server
    let stream = UnixStream::connect(&config.socket_path)
        .await
        .context("Failed to connect to AgentLoop server. Is the server running?")?;

    let (reader, writer) = stream.into_split();
    let reader = BufReader::new(reader);
    let writer = Arc::new(tokio::sync::Mutex::new(writer));

    // Determine command to execute
    let command = if let Some(cmd) = cli.command {
        cmd
    } else if let Some(task) = cli.task {
        Commands::Task {
            description: task,
            work_dir: cli.work_dir,
        }
    } else {
        anyhow::bail!("No task or command provided. Use --help for usage information.");
    };

    // Execute command
    match command {
        Commands::Task { description, work_dir } => {
            run_task(reader, writer, &cli.user, &description, work_dir).await?;
        }
        Commands::Sessions { user } => {
            list_sessions(reader, writer, user.as_deref()).await?;
        }
        Commands::Health => {
            check_health(reader, writer).await?;
        }
        Commands::Abort { session_id } => {
            abort_session(reader, writer, &session_id).await?;
        }
        Commands::Memory { user } => {
            get_memory(reader, writer, &user).await?;
        }
    }

    Ok(())
}

/// Run a new agent task
async fn run_task(
    mut reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
    user_id: &str,
    task: &str,
    work_dir: Option<String>,
) -> Result<()> {
    info!("Starting task: {}", task);

    // Send task.start request
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "task.start".to_string(),
        params: json!({
            "userId": user_id,
            "text": task,
            "workDir": work_dir,
            "source": "cli"
        }),
        id: 1,
    };

    send_request(&writer, &request).await?;

    // Handle responses and events
    let mut session_id: Option<String> = None;
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);

    // Handle Ctrl+C
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        warn!("Received interrupt signal");
        shutdown_clone.store(true, Ordering::Relaxed);
    });

    let mut line = String::new();
    while !shutdown.load(Ordering::Relaxed) {
        line.clear();
        match reader.read_line(&mut line).await? {
            0 => break, // EOF
            _ => {
                if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&line.trim()) {
                    if let Some(error) = response.error {
                        error!("Server error: {} (code: {})", error.message, error.code);
                        return Err(anyhow::anyhow!("Server returned error: {}", error.message));
                    }
                    
                    if let Some(result) = response.result {
                        if let Some(sid) = result.get("sessionId").and_then(|v| v.as_str()) {
                            session_id = Some(sid.to_string());
                            info!("Session started: {}", sid);
                        }
                    }
                } else if let Ok(notification) = serde_json::from_str::<JsonRpcNotification>(&line.trim()) {
                    handle_event(&notification)?;
                    
                    // Check if task is done
                    if notification.method == "event.done" || notification.method == "event.error" {
                        break;
                    }
                }
            }
        }
    }

    // If interrupted, abort the session
    if shutdown.load(Ordering::Relaxed) {
        if let Some(sid) = session_id {
            warn!("Aborting session due to interrupt: {}", sid);
            let abort_request = JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                method: "task.abort".to_string(),
                params: json!({
                    "sessionId": sid
                }),
                id: 2,
            };
            send_request(&writer, &abort_request).await?;
        }
    }

    Ok(())
}

/// Handle incoming events from the server
fn handle_event(notification: &JsonRpcNotification) -> Result<()> {
    match notification.method.as_str() {
        "event.text" => {
            if let Some(content) = notification.params.get("content").and_then(|v| v.as_str()) {
                print!("{}", content);
                std::io::stdout().flush().ok();
            }
        }
        "event.tool_use" => {
            if let (Some(tool_name), Some(input)) = (
                notification.params.get("toolName").and_then(|v| v.as_str()),
                notification.params.get("input")
            ) {
                println!("🔧 Using tool: {} with input: {}", tool_name, input);
            }
        }
        "event.tool_result" => {
            if let (Some(tool_name), Some(success)) = (
                notification.params.get("toolName").and_then(|v| v.as_str()),
                notification.params.get("success").and_then(|v| v.as_bool())
            ) {
                let status = if success { "✅" } else { "❌" };
                println!("{} Tool {} completed", status, tool_name);
            }
        }
        "event.hitl_request" => {
            if let (Some(tool_name), Some(details)) = (
                notification.params.get("toolName").and_then(|v| v.as_str()),
                notification.params.get("details").and_then(|v| v.as_str())
            ) {
                println!("🤔 HITL Request for tool {}: {}", tool_name, details);
                println!("   (This would normally prompt for approval)");
            }
        }
        "event.done" => {
            println!("✅ Task completed successfully!");
        }
        "event.error" => {
            if let Some(message) = notification.params.get("message").and_then(|v| v.as_str()) {
                println!("❌ Task failed: {}", message);
            } else {
                println!("❌ Task failed with unknown error");
            }
        }
        "event.session_saved" => {
            println!("💾 Session saved to vault");
        }
        _ => {
            // Unknown event type, ignore
        }
    }
    Ok(())
}

/// List active sessions
async fn list_sessions(
    mut reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
    user: Option<&str>,
) -> Result<()> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session.list".to_string(),
        params: json!({
            "userId": user
        }),
        id: 1,
    };

    send_request(&writer, &request).await?;

    let mut line = String::new();
    reader.read_line(&mut line).await?;
    
    let response: JsonRpcResponse = serde_json::from_str(&line.trim())?;
    if let Some(error) = response.error {
        return Err(anyhow::anyhow!("Server error: {}", error.message));
    }

    if let Some(sessions) = response.result {
        if let Some(sessions_array) = sessions.as_array() {
            if sessions_array.is_empty() {
                println!("No active sessions found.");
            } else {
                println!("Active sessions:");
                for session in sessions_array {
                    if let (Some(id), Some(state), Some(task)) = (
                        session.get("id").and_then(|v| v.as_str()),
                        session.get("state").and_then(|v| v.as_str()),
                        session.get("task").and_then(|v| v.as_str())
                    ) {
                        println!("  {} [{}]: {}", id, state, &task[..task.len().min(50)]);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Check server health
async fn check_health(
    mut reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
) -> Result<()> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "health.check".to_string(),
        params: json!({}),
        id: 1,
    };

    send_request(&writer, &request).await?;

    let mut line = String::new();
    reader.read_line(&mut line).await?;
    
    let response: JsonRpcResponse = serde_json::from_str(&line.trim())?;
    if let Some(error) = response.error {
        return Err(anyhow::anyhow!("Server error: {}", error.message));
    }

    if let Some(result) = response.result {
        println!("Server health: {}", serde_json::to_string_pretty(&result)?);
    }

    Ok(())
}

/// Abort a session
async fn abort_session(
    mut reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
    session_id: &str,
) -> Result<()> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "task.abort".to_string(),
        params: json!({
            "sessionId": session_id
        }),
        id: 1,
    };

    send_request(&writer, &request).await?;

    let mut line = String::new();
    reader.read_line(&mut line).await?;
    
    let response: JsonRpcResponse = serde_json::from_str(&line.trim())?;
    if let Some(error) = response.error {
        return Err(anyhow::anyhow!("Server error: {}", error.message));
    }

    println!("Session {} aborted", session_id);
    Ok(())
}

/// Get memory context for user
async fn get_memory(
    mut reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
    user_id: &str,
) -> Result<()> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "memory.get".to_string(),
        params: json!({
            "userId": user_id
        }),
        id: 1,
    };

    send_request(&writer, &request).await?;

    let mut line = String::new();
    reader.read_line(&mut line).await?;
    
    let response: JsonRpcResponse = serde_json::from_str(&line.trim())?;
    if let Some(error) = response.error {
        return Err(anyhow::anyhow!("Server error: {}", error.message));
    }

    if let Some(result) = response.result {
        println!("Memory context for {}:", user_id);
        println!("{}", serde_json::to_string_pretty(&result)?);
    }

    Ok(())
}

/// Send a JSON-RPC request
async fn send_request(
    writer: &Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
    request: &JsonRpcRequest,
) -> Result<()> {
    let json = serde_json::to_string(request)?;
    let line = json + "\n";
    
    let mut writer = writer.lock().await;
    writer.write_all(line.as_bytes()).await?;
    Ok(())
}