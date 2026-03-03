//! UNIX socket server with JSON-RPC 2.0 support
//! 
//! Equivalent to Go version's internal/server package.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, RwLock, Mutex};
use uuid::Uuid;

use crate::{Result, AgentLoopError, session::{SessionManager, SessionMessage, HITLDecision}};

/// JSON-RPC 2.0 request
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
    pub id: Option<Value>,
}

/// JSON-RPC 2.0 response
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
}

/// JSON-RPC 2.0 error
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC 2.0 notification (no id)
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
}

/// Task start parameters
#[derive(Debug, Deserialize)]
pub struct TaskStartParams {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub text: String,
    #[serde(rename = "workDir")]
    pub work_dir: Option<String>,
    pub source: String,
}

/// Task steer parameters
#[derive(Debug, Deserialize)]
pub struct TaskSteerParams {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub text: String,
}

/// Task abort parameters
#[derive(Debug, Deserialize)]
pub struct TaskAbortParams {
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

/// HITL respond parameters
#[derive(Debug, Deserialize)]
pub struct HITLRespondParams {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "requestId")]
    pub request_id: String,
    pub decision: HITLDecision,
}

/// Session list parameters
#[derive(Debug, Deserialize)]
pub struct SessionListParams {
    #[serde(rename = "userId")]
    pub user_id: Option<String>,
    pub status: Option<String>,
}

/// Memory get parameters
#[derive(Debug, Deserialize)]
pub struct MemoryGetParams {
    #[serde(rename = "userId")]
    pub user_id: String,
}

/// Health check parameters (empty)
#[derive(Debug, Deserialize)]
pub struct HealthCheckParams {}

/// Server event types for broadcasting
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "method", content = "params")]
pub enum ServerEvent {
    #[serde(rename = "event.text")]
    Text {
        #[serde(rename = "sessionId")]
        session_id: String,
        content: String,
    },
    #[serde(rename = "event.tool_use")]
    ToolUse {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        input: Value,
    },
    #[serde(rename = "event.tool_result")]
    ToolResult {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        output: String,
        success: bool,
    },
    #[serde(rename = "event.hitl_request")]
    HITLRequest {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        details: String,
        options: Vec<String>,
    },
    #[serde(rename = "event.done")]
    Done {
        #[serde(rename = "sessionId")]
        session_id: String,
        output: String,
        stats: Value,
    },
    #[serde(rename = "event.error")]
    Error {
        #[serde(rename = "sessionId")]
        session_id: String,
        message: String,
    },
    #[serde(rename = "event.session_saved")]
    SessionSaved {
        #[serde(rename = "sessionId")]
        session_id: String,
    },
}

/// Client connection
#[derive(Debug)]
pub struct Client {
    pub id: String,
    pub subscriptions: Arc<RwLock<Vec<String>>>, // session IDs
    pub event_tx: broadcast::Sender<ServerEvent>,
}

/// AgentLoop server
pub struct Server {
    listener: UnixListener,
    session_manager: Arc<SessionManager>,
    clients: Arc<RwLock<HashMap<String, Client>>>,
    broadcast_tx: broadcast::Sender<ServerEvent>,
    _broadcast_rx: broadcast::Receiver<ServerEvent>, // Keep one receiver to prevent channel close
}

impl Server {
    /// Create new server
    pub async fn new(socket_path: &std::path::Path, session_manager: SessionManager) -> Result<Self> {
        // Remove existing socket file if it exists
        if socket_path.exists() {
            tokio::fs::remove_file(socket_path).await?;
        }

        // Create parent directory if needed
        if let Some(parent) = socket_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Create listener
        let listener = UnixListener::bind(socket_path)?;

        // Set permissions to 0700 (owner only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o700);
            std::fs::set_permissions(socket_path, perms)?;
        }

        let (broadcast_tx, broadcast_rx) = broadcast::channel(1000);

        Ok(Self {
            listener,
            session_manager: Arc::new(session_manager),
            clients: Arc::new(RwLock::new(HashMap::new())),
            broadcast_tx,
            _broadcast_rx: broadcast_rx,
        })
    }

    /// Run the server
    pub async fn run(&self) -> Result<()> {
        tracing::info!("AgentLoop server starting on {:?}", self.listener.local_addr()?);

        loop {
            match self.listener.accept().await {
                Ok((stream, _)) => {
                    let client_id = Uuid::new_v4().to_string();
                    tracing::debug!("New client connected: {}", client_id);

                    let client = Client {
                        id: client_id.clone(),
                        subscriptions: Arc::new(RwLock::new(Vec::new())),
                        event_tx: self.broadcast_tx.clone(),
                    };

                    self.clients.write().await.insert(client_id.clone(), client);

                    // Spawn handler for this client
                    let clients = Arc::clone(&self.clients);
                    let session_manager = Arc::clone(&self.session_manager);
                    let event_rx = self.broadcast_tx.subscribe();

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_client(stream, client_id.clone(), clients.clone(), session_manager, event_rx).await {
                            tracing::warn!("Client {} error: {}", client_id, e);
                        }
                        
                        // Remove client on disconnect
                        clients.write().await.remove(&client_id);
                        tracing::debug!("Client {} disconnected", client_id);
                    });
                }
                Err(e) => {
                    tracing::error!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    /// Handle a single client connection
    async fn handle_client(
        stream: UnixStream,
        client_id: String,
        clients: Arc<RwLock<HashMap<String, Client>>>,
        session_manager: Arc<SessionManager>,
        mut event_rx: broadcast::Receiver<ServerEvent>,
    ) -> Result<()> {
        let (reader, writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        // Spawn task to handle outgoing events
        let clients_clone = Arc::clone(&clients);
        let client_id_clone = client_id.clone();
        let writer_shared = Arc::new(Mutex::new(writer));
        let writer_for_events = Arc::clone(&writer_shared);

        tokio::spawn(async move {
            loop {
                match event_rx.recv().await {
                    Ok(event) => {
                        // Check if client is subscribed to this session
                        let should_send = {
                            let clients = clients_clone.read().await;
                            if let Some(client) = clients.get(&client_id_clone) {
                                let subscriptions = client.subscriptions.read().await;
                                match &event {
                                    ServerEvent::Text { session_id, .. } |
                                    ServerEvent::ToolUse { session_id, .. } |
                                    ServerEvent::ToolResult { session_id, .. } |
                                    ServerEvent::HITLRequest { session_id, .. } |
                                    ServerEvent::Done { session_id, .. } |
                                    ServerEvent::Error { session_id, .. } |
                                    ServerEvent::SessionSaved { session_id, .. } => {
                                        subscriptions.contains(session_id)
                                    }
                                }
                            } else {
                                false
                            }
                        };

                        if should_send {
                            let notification = JsonRpcNotification {
                                jsonrpc: "2.0".to_string(),
                                method: match &event {
                                    ServerEvent::Text { .. } => "event.text",
                                    ServerEvent::ToolUse { .. } => "event.tool_use",
                                    ServerEvent::ToolResult { .. } => "event.tool_result",
                                    ServerEvent::HITLRequest { .. } => "event.hitl_request",
                                    ServerEvent::Done { .. } => "event.done",
                                    ServerEvent::Error { .. } => "event.error",
                                    ServerEvent::SessionSaved { .. } => "event.session_saved",
                                }.to_string(),
                                params: serde_json::to_value(&event).unwrap_or(Value::Null),
                            };

                            let mut writer = writer_for_events.lock().await;
                            let line = serde_json::to_string(&notification).unwrap() + "\n";
                            if let Err(e) = writer.write_all(line.as_bytes()).await {
                                tracing::warn!("Failed to send event to client {}: {}", client_id_clone, e);
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        tracing::warn!("Client {} event queue lagged", client_id_clone);
                        continue;
                    }
                }
            }
        });

        // Handle incoming requests
        loop {
            line.clear();
            match reader.read_line(&mut line).await? {
                0 => break, // EOF
                _ => {
                    if let Err(e) = Self::handle_request(&line, &client_id, &clients, &session_manager, &writer_shared).await {
                        tracing::warn!("Failed to handle request from client {}: {}", client_id, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle a single JSON-RPC request
    async fn handle_request(
        line: &str,
        client_id: &str,
        clients: &Arc<RwLock<HashMap<String, Client>>>,
        session_manager: &Arc<SessionManager>,
        writer: &Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    ) -> Result<()> {
        let request: JsonRpcRequest = serde_json::from_str(line.trim())?;
        
        tracing::debug!("Handling request: {} from client {}", request.method, client_id);

        let response = match request.method.as_str() {
            "task.start" => Self::handle_task_start(request.params, client_id, clients, session_manager).await,
            "task.steer" => Self::handle_task_steer(request.params, session_manager).await,
            "task.abort" => Self::handle_task_abort(request.params, session_manager).await,
            "hitl.respond" => Self::handle_hitl_respond(request.params, session_manager).await,
            "session.list" => Self::handle_session_list(request.params, session_manager).await,
            "memory.get" => Self::handle_memory_get(request.params).await,
            "health.check" => Self::handle_health_check(session_manager).await,
            _ => Err(AgentLoopError::fatal(format!("Unknown method: {}", request.method))),
        };

        let json_response = match response {
            Ok(result) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: Some(result),
                error: None,
                id: request.id,
            },
            Err(e) => {
                let (code, message) = match &e {
                    AgentLoopError::Fatal { message, .. } => (-32603, message.clone()),
                    AgentLoopError::Retryable { message, .. } => (-32002, message.clone()),
                    AgentLoopError::UserAbort { message } => (-32001, message.clone()),
                    AgentLoopError::ToolFailure { message, .. } => (-32004, message.clone()),
                    _ => (-32603, e.to_string()),
                };

                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    result: None,
                    error: Some(JsonRpcError {
                        code,
                        message,
                        data: None,
                    }),
                    id: request.id,
                }
            }
        };

        let mut writer = writer.lock().await;
        let response_line = serde_json::to_string(&json_response)? + "\n";
        writer.write_all(response_line.as_bytes()).await?;

        Ok(())
    }

    /// Handle task.start method
    async fn handle_task_start(
        params: Value,
        client_id: &str,
        clients: &Arc<RwLock<HashMap<String, Client>>>,
        session_manager: &Arc<SessionManager>,
    ) -> Result<Value> {
        let params: TaskStartParams = serde_json::from_value(params)?;
        
        let (session_id, _message_rx) = session_manager
            .start_session(params.user_id, params.text, params.work_dir, params.source)
            .await?;

        // Subscribe client to session events
        {
            let clients = clients.read().await;
            if let Some(client) = clients.get(client_id) {
                let mut subscriptions = client.subscriptions.write().await;
                subscriptions.push(session_id.clone());
            }
        }

        // TODO: Start actual agent processing in background
        // For now, just return the session ID

        Ok(json!({
            "sessionId": session_id
        }))
    }

    /// Handle task.steer method
    async fn handle_task_steer(params: Value, session_manager: &Arc<SessionManager>) -> Result<Value> {
        let params: TaskSteerParams = serde_json::from_value(params)?;
        
        session_manager
            .send_message(&params.session_id, SessionMessage::Steer(params.text))
            .await?;

        Ok(json!({}))
    }

    /// Handle task.abort method
    async fn handle_task_abort(params: Value, session_manager: &Arc<SessionManager>) -> Result<Value> {
        let params: TaskAbortParams = serde_json::from_value(params)?;
        
        session_manager
            .send_message(&params.session_id, SessionMessage::Abort)
            .await?;

        Ok(json!({}))
    }

    /// Handle hitl.respond method
    async fn handle_hitl_respond(params: Value, session_manager: &Arc<SessionManager>) -> Result<Value> {
        let params: HITLRespondParams = serde_json::from_value(params)?;
        
        session_manager
            .resolve_hitl(&params.session_id, &params.request_id, params.decision)
            .await?;

        Ok(json!({}))
    }

    /// Handle session.list method
    async fn handle_session_list(params: Value, session_manager: &Arc<SessionManager>) -> Result<Value> {
        let params: SessionListParams = serde_json::from_value(params)?;
        
        if let Some(user_id) = params.user_id {
            let sessions = session_manager.list_user_sessions(&user_id).await?;
            Ok(serde_json::to_value(sessions)?)
        } else {
            // TODO: List all sessions (admin feature)
            Ok(json!([]))
        }
    }

    /// Handle memory.get method
    async fn handle_memory_get(_params: Value) -> Result<Value> {
        // TODO: Implement memory retrieval
        Ok(json!({
            "context": "",
            "facts": {},
            "preferences": {}
        }))
    }

    /// Handle health.check method
    async fn handle_health_check(session_manager: &Arc<SessionManager>) -> Result<Value> {
        let session_count = session_manager.session_count().await;
        
        Ok(json!({
            "status": "healthy",
            "sessionCount": session_count,
            "timestamp": chrono::Utc::now()
        }))
    }

    /// Broadcast event to all subscribed clients
    pub fn broadcast_event(&self, event: ServerEvent) {
        if let Err(e) = self.broadcast_tx.send(event) {
            tracing::warn!("Failed to broadcast event: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SessionConfig;
    use tempfile::TempDir;

    fn test_session_config() -> SessionConfig {
        SessionConfig {
            max_concurrent: 10,
            max_per_user: 5,
            timeout_seconds: 3600,
            token_budget: 100000,
            tool_call_limit: 1000,
            stuck_threshold_seconds: 300,
            evict_lru: true,
        }
    }

    #[tokio::test]
    async fn test_server_creation() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");
        
        let session_manager = SessionManager::new(test_session_config());
        let server = Server::new(&socket_path, session_manager).await.unwrap();
        
        assert!(socket_path.exists());
        
        // Check permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&socket_path).unwrap();
            let permissions = metadata.permissions();
            assert_eq!(permissions.mode() & 0o777, 0o700);
        }
    }

    #[tokio::test]
    async fn test_json_rpc_request_parsing() {
        let json = r#"{"jsonrpc":"2.0","method":"health.check","params":{},"id":1}"#;
        let request: JsonRpcRequest = serde_json::from_str(json).unwrap();
        
        assert_eq!(request.jsonrpc, "2.0");
        assert_eq!(request.method, "health.check");
        assert_eq!(request.id, Some(json!(1)));
    }

    #[tokio::test]
    async fn test_json_rpc_response_serialization() {
        let response = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: Some(json!({"status": "ok"})),
            error: None,
            id: Some(json!(1)),
        };
        
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"result\":{\"status\":\"ok\"}"));
        assert!(!json.contains("\"error\""));
    }
}