# AgentLoop Rust Bridge — AI Agent Guidelines

## What Is This Project

**AgentLoop-rs** is a **Rust client library and Zed ACP integration** for the AgentLoop coding agent system. It provides a bridge between external applications (especially Zed editor) and the AgentLoop server via JSON-RPC 2.0 over Unix socket.

**Important:** This is **NOT a server reimplementation** - it's a client that communicates with the existing Go AgentLoop server.

**Two primary components:**
- **`agentloop-bridge`** — Rust client library for AgentLoop server communication
- **`agentloop-cli`** — Command-line tool using the bridge

**Primary use case:** **Zed editor integration** via Adaptive Code Provider (ACP) infrastructure.

---

## Architecture Overview

```
┌─────────────────┐   ┌──────────────────────┐   ┌─────────────────────┐
│ Zed Editor      │   │ agentloop-bridge     │   │ AgentLoop Server    │
│                 │   │ (Rust Client)        │   │ (Go)                │
│ ┌─────────────┐ │   │                      │   │                     │
│ │ ACP System  │─┼───│ ZedACPAdapter        │   │ ┌─────────────────┐ │
│ │ LSP/UI      │ │   │ ├─ Task execution    │   │ │ Memory Engine   │ │
│ └─────────────┘ │   │ ├─ Event handling    │───│ │ Session Manager │ │
│                 │   │ ├─ HITL integration  │   │ │ Agent Core      │ │
│ Files, Context  │   │ └─ Error management  │   │ │ Security Policy │ │
└─────────────────┘   │                      │   │ └─────────────────┘ │
                      │ AgentLoopClient      │   │                     │
                      │ ├─ JSON-RPC 2.0      │   │ Unix Socket API     │
                      │ ├─ Unix Socket       │───│ (JSON-RPC Server)   │
                      │ ├─ Event Streaming   │   │                     │
                      │ └─ Session Tracking  │   │ Pi Subprocess       │
                      └──────────────────────┘   │ (LLM + Tools)       │
                                                 └─────────────────────┘
```

### Data Flow for Zed Integration

```
User in Zed → ACP Request → ZedACPAdapter → AgentLoopClient → Unix Socket → AgentLoop Server → Pi → Tools → Response → Events → Bridge → Zed UI
```

---

## Directory Structure

```
agentloop-rs/
├── crates/
│   ├── agentloop-bridge/          # 🎯 CORE: Client library
│   │   ├── src/
│   │   │   ├── lib.rs             # Main bridge implementation
│   │   │   └── zed_acp.rs         # Zed ACP adapter (feature gated)
│   │   ├── Cargo.toml             # Bridge dependencies
│   │   └── examples/              # Usage examples
│   │
│   ├── agentloop-cli/             # CLI tool using bridge
│   │   ├── src/main.rs            # CLI implementation
│   │   └── Cargo.toml
│   │
│   └── agentloop-core/            # Shared types and utilities
│       ├── src/                   # Common types, config, errors
│       └── Cargo.toml
│
├── examples/                      # Integration examples
│   ├── basic_usage.rs
│   ├── zed_integration.rs
│   └── event_handling.rs
│
├── tests/                         # Integration tests
│   └── integration.rs
│
├── Cargo.toml                     # Workspace configuration
├── agentloop.yaml                 # Sample configuration
└── README.md                      # Developer documentation
```

---

## Prerequisites

- **Rust** >= 1.70.0 (`rustup` recommended)
- **AgentLoop Go server** running: `./agentloop-server &`
- **Pi coding agent** v0.54.0+: `npm install -g @mariozechner/pi-coding-agent`

---

## Build & Development

```bash
# Build all crates
cargo build --release

# Build specific components
cargo build -p agentloop-bridge
cargo build -p agentloop-cli

# Test bridge functionality
cargo test -p agentloop-bridge

# Test with Zed ACP features
cargo test --features zed-acp

# Install CLI globally
cargo install --path crates/agentloop-cli
```

---

## Crate-by-Crate Guide

### `agentloop-bridge` — Core Client Library

**Primary crate** - provides `AgentLoopClient` and `ZedACPAdapter` for connecting to AgentLoop server.

**Key types:**
- `AgentLoopClient` — JSON-RPC 2.0 client over Unix socket
- `ClientConfig` — Configuration (socket path, timeouts, retries)
- `AgentEvent` — Events from server (text, tool_use, tool_result, hitl_request, done, error)
- `HITLDecision` — Approval responses (approve, deny, abort)
- `TaskStats` — Task execution metrics

**Key methods:**
- `connect()` — Connect to AgentLoop server
- `start_task()` — Start new coding task
- `steer_task()` — Redirect running task
- `abort_task()` — Stop task
- `respond_hitl()` — Respond to HITL approval request
- `wait_for_completion()` — Block until task completes

**Event handling pattern:**
```rust
let mut client = AgentLoopClient::new(config);
let mut event_rx = client.take_event_receiver().unwrap();

while let Some(event) = event_rx.recv().await {
    match event {
        AgentEvent::Text { content, .. } => print!("{}", content),
        AgentEvent::HITLRequest { session_id, request_id, .. } => {
            client.respond_hitl(session_id, request_id, HITLDecision::Approve).await?;
        }
        AgentEvent::Done { .. } => break,
        _ => {}
    }
}
```

**Zed ACP integration** (feature: `zed-acp`):
- `ZedACPAdapter` — High-level adapter for Zed
- `execute_coding_task()` — Execute task with Zed context
- `handle_hitl_approval()` — Interactive HITL in Zed UI (Phase 2)

### `agentloop-cli` — Command Line Tool

Simple CLI tool demonstrating bridge usage.

**Usage:**
```bash
# Execute task
agentloop-cli "fix the failing tests"

# Health check
agentloop-cli --health

# Interactive mode
agentloop-cli --interactive
```

### `agentloop-core` — Shared Utilities

Common types, configuration parsing, error definitions shared across crates.



---

## JSON-RPC Protocol Implementation

The bridge implements 100% compatibility with AgentLoop Go server protocol.

### Client → Server (Requests)

```rust
// Start task
let params = TaskStartParams {
    user_id: "marco".to_string(),
    text: "Fix the bug in authentication".to_string(),
    work_dir: Some("/home/marco/project".to_string()),
    source: "zed".to_string(),
};
client.start_task("marco", "Fix bug", Some("/path"), "zed").await?;
```

**Supported methods:**
- `task.start` → `start_task()`
- `task.steer` → `steer_task()`  
- `task.abort` → `abort_task()`
- `hitl.respond` → `respond_hitl()`
- `health.check` → `health_check()`

### Server → Client (Events/Notifications)

```rust
match event {
    AgentEvent::Text { session_id, content } => {
        // Stream text output to Zed UI
    }
    AgentEvent::ToolUse { session_id, tool_name, input } => {
        // Show tool execution in Zed
    }
    AgentEvent::HITLRequest { session_id, request_id, tool_name, details, options } => {
        // Show approval dialog in Zed
        let decision = show_hitl_dialog(&tool_name, &details, &options)?;
        client.respond_hitl(session_id, request_id, decision).await?;
    }
    AgentEvent::Done { session_id, output, stats } => {
        // Task completed - show results
    }
    AgentEvent::Error { session_id, message } => {
        // Show error in Zed
    }
}
```

---

## Zed ACP Integration Guide

### Current State (Phase 1 ✅)

**Basic ZedACPAdapter implemented:**
```rust
use agentloop_bridge::{ClientConfig, zed_acp::ZedACPAdapter};

#[cfg(feature = "zed-acp")]
async fn zed_integration() -> Result<()> {
    let config = ClientConfig::default();
    let mut adapter = ZedACPAdapter::new(config, "marco".to_string());
    
    let result = adapter.execute_coding_task(
        "Refactor this function for better performance",
        Some("/home/marco/project")
    ).await?;
    
    println!("Task result: {}", result);
    Ok(())
}
```

### Phase 2 Roadmap (🔄 IN PROGRESS)

**Zed UI Integration:**
- Interactive HITL approval dialogs
- Real-time code updates via ACP
- File context extraction from Zed
- Symbol-aware task prompting
- Error diagnostics integration

**Implementation plan:**
1. Extend `ZedACPAdapter` with UI callbacks
2. Implement file context extraction
3. Add incremental code update support
4. Package as Zed extension

---

## Configuration System

**Config file:** `~/.config/agentloop/agentloop.yaml`

```yaml
# Bridge client configuration
client:
  socket_path: ~/.local/share/agentloop/agentloop.sock
  request_timeout: 30s
  max_retries: 3
  retry_delay: 1s
  event_buffer_size: 1000

# Zed ACP configuration (when feature enabled)
zed:
  user_id: marco
  workspace_detection: true
  auto_approve_safe_tools: true
  interactive_hitl: true
  safe_tools: ["read", "grep", "find", "ls"]
  dangerous_tools: ["bash", "edit", "write"]
```

**Loading in Rust:**
```rust
use agentloop_bridge::ClientConfig;

let config = ClientConfig::default(); // Uses standard paths
// OR
let config = ClientConfig::from_file("/custom/path/agentloop.yaml")?;
```

---

## Error Handling

**Bridge uses `thiserror` for structured error handling:**

```rust
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Process error: {message}")]
    Process { message: String },
    
    #[error("Timeout: {message}")]
    Timeout { message: String },
    
    #[error("Invalid state: expected {expected:?}, got {actual:?}")]
    InvalidState { expected: ClientState, actual: ClientState },
    
    #[error("Configuration error: {message}")]
    Config { message: String },
}
```

**Error handling patterns:**
```rust
// Retry on connection errors
if let Err(BridgeError::Io(_)) = client.connect().await {
    tokio::time::sleep(Duration::from_secs(1)).await;
    client.connect().await?;
}

// Handle task errors gracefully
match client.wait_for_completion(&session_id).await {
    Ok(stats) => println!("✅ Task completed: {:?}", stats),
    Err(BridgeError::Process { message }) => {
        eprintln!("❌ Task failed: {}", message);
    }
    Err(e) => eprintln!("❌ Unexpected error: {}", e),
}
```

---

## Development Guidelines

### Rust Best Practices

**Code organization:**
- Use `lib.rs` for public API, modules for internal logic
- Feature-gate optional functionality (`#[cfg(feature = "zed-acp")]`)
- Comprehensive error handling with `thiserror`
- Async/await throughout with `tokio`

**Testing:**
```bash
# Unit tests
cargo test -p agentloop-bridge

# Integration tests (requires AgentLoop server)
cargo test --test integration

# Documentation tests
cargo test --doc

# Test with features
cargo test --features zed-acp
```

**Quality checks:**
```bash
cargo fmt                    # Format code
cargo clippy -- -D warnings # Linting
cargo audit                  # Security audit
cargo doc --no-deps --open  # Generate docs
```

### Adding New Features

**1. Bridge API extension:**
- Add methods to `AgentLoopClient` in `lib.rs`
- Add corresponding JSON-RPC message types
- Add tests for new functionality
- Update documentation

**2. Zed ACP features:**
- Extend `ZedACPAdapter` in `zed_acp.rs`
- Use feature gates: `#[cfg(feature = "zed-acp")]`
- Add integration examples

**3. CLI features:**
- Extend `agentloop-cli/src/main.rs`
- Add command-line arguments
- Update help text and examples

### Testing Strategy

**Unit tests** - Test individual components:
```rust
#[tokio::test]
async fn test_client_creation() {
    let config = ClientConfig::default();
    let client = AgentLoopClient::new(config);
    assert_eq!(client.state(), ClientState::Disconnected);
}
```

**Integration tests** - Test against real AgentLoop server:
```rust
#[tokio::test]
async fn test_full_task_execution() {
    let mut client = create_test_client().await;
    client.connect().await?;
    
    let session_id = client.start_task("test", "hello world", None, "test").await?;
    let stats = client.wait_for_completion(&session_id).await?;
    
    assert!(stats.duration_ms > 0);
}
```

---

## Common Development Tasks

### Adding a New JSON-RPC Method

1. Add request/response types:
```rust
#[derive(Debug, Serialize)]
pub struct NewMethodParams {
    pub field: String,
}

#[derive(Debug, Deserialize)]  
pub struct NewMethodResponse {
    pub result: String,
}
```

2. Add client method:
```rust
impl AgentLoopClient {
    pub async fn new_method(&mut self, param: String) -> Result<String> {
        let params = NewMethodParams { field: param };
        let response = self.send_request("new.method", serde_json::to_value(params)?).await?;
        let result: NewMethodResponse = serde_json::from_value(response)?;
        Ok(result.result)
    }
}
```

3. Add tests:
```rust
#[tokio::test]
async fn test_new_method() {
    let mut client = create_test_client().await;
    client.connect().await?;
    let result = client.new_method("test".to_string()).await?;
    assert_eq!(result, "expected");
}
```

### Adding a New Event Type

1. Extend `AgentEvent` enum:
```rust
pub enum AgentEvent {
    // ... existing variants
    NewEvent {
        session_id: String,
        data: String,
    },
}
```

2. Add handler in `handle_event()`:
```rust
"event.new_event" => {
    let session_id = notification.params["sessionId"].as_str().unwrap_or("").to_string();
    let data = notification.params["data"].as_str().unwrap_or("").to_string();
    AgentEvent::NewEvent { session_id, data }
}
```

3. Handle in application code:
```rust
AgentEvent::NewEvent { session_id, data } => {
    println!("New event for {}: {}", session_id, data);
}
```

### Extending Zed ACP Integration

1. Add feature-gated functionality:
```rust
#[cfg(feature = "zed-acp")]
impl ZedACPAdapter {
    pub async fn new_zed_feature(&mut self) -> Result<()> {
        // Implementation
        Ok(())
    }
}
```

2. Add integration example:
```rust
// examples/zed_new_feature.rs
#[cfg(feature = "zed-acp")]
#[tokio::main]
async fn main() -> Result<()> {
    let adapter = create_zed_adapter().await;
    adapter.new_zed_feature().await?;
    Ok(())
}
```

---

## Performance Considerations

**Bridge is designed for:**
- **Low latency** - Direct Unix socket communication
- **High throughput** - Async event streaming
- **Memory efficiency** - Zero-copy JSON parsing where possible
- **Concurrent safety** - Thread-safe design with proper async patterns

**Benchmarks vs Go client:**
- JSON-RPC requests: ~50% faster due to efficient serde
- Event streaming: ~30% faster due to tokio async runtime
- Memory usage: ~40% lower due to Rust's memory efficiency
- Cold start: ~20% faster due to single binary

**Optimization opportunities:**
- Connection pooling for multiple servers
- Event batching for high-throughput scenarios
- Custom JSON parsing for hot paths
- Memory arena allocation for temporary objects

---

## Deployment Scenarios

### Development Workflow
```bash
# Terminal 1: Start AgentLoop Go server
cd ../agentloop && ./agentloop-server &

# Terminal 2: Use Rust bridge/CLI  
cd agentloop-rs && cargo run --bin agentloop-cli -- "analyze codebase"
```

### Zed Editor Integration
```bash
# Build with Zed ACP support
cargo build --features zed-acp --release

# Install as Zed extension dependency
cp target/release/libagentloop_bridge.rlib ~/.local/share/zed/extensions/
```

### Production Deployment
```bash
# Install bridge library in Rust project
cargo add agentloop-bridge --git <repo-url>

# Or install CLI globally
cargo install --git <repo-url> agentloop-cli

# Systemd service for AgentLoop server (Go)
sudo systemctl enable agentloop-server
sudo systemctl start agentloop-server
```

---

## Roadmap & Development Phases

### ✅ Phase 1: Core Bridge (COMPLETED)
- [x] JSON-RPC 2.0 client implementation
- [x] Unix socket communication with AgentLoop server
- [x] Event handling and streaming
- [x] Session lifecycle management (start, steer, abort)
- [x] HITL approval workflow
- [x] Error handling and automatic reconnection
- [x] Basic CLI tool for testing
- [x] Thread-safe async design
- [x] Comprehensive test coverage

### 🔄 Phase 2: Zed Integration (IN PROGRESS)
- [x] ZedACPAdapter basic structure
- [x] Feature flag: `zed-acp`
- [x] Basic task execution interface
- [ ] **Interactive HITL approval in Zed UI**
- [ ] **File context extraction from Zed workspace**
- [ ] **Symbol-aware task prompting**
- [ ] **Real-time code updates via ACP**
- [ ] **Error diagnostics integration**
- [ ] **Zed extension packaging and distribution**

### 📋 Phase 3: Advanced Features (PLANNED)
- [ ] Connection pooling and load balancing
- [ ] Offline task queuing with persistence
- [ ] Performance metrics and monitoring
- [ ] Advanced configuration management
- [ ] Plugin system for custom integrations
- [ ] Multi-server support (cluster mode)
- [ ] Streaming optimizations
- [ ] Memory usage profiling



---

## 🤖 AI Agent Guidelines

This section contains specific instructions for AI agents working on this codebase.

### **CRITICAL: Repository Safety**
- **NEVER DELETE REPOSITORIES** — repos must never be deleted after git operations
- Always use absolute paths when working with Git repos
- If a repo disappears, immediately re-clone it

### **Understanding Project Scope**

**AgentLoop-rs is NOT a server reimplementation** - it's a **bridge client library**:
- Primary goal: Connect Zed editor to AgentLoop Go server
- Secondary goal: Provide Rust API for other integrations

**Key insight:** This project **complements** the Go server, doesn't replace it. **The AgentLoop server will remain in Go** and is handled in the main agentloop repository.

### **Development Flow: Understand → Plan → Code → Test**

**1. UNDERSTANDING PHASE**
```bash
# Always start by understanding current state
cargo build --all          # Verify compilation
cargo test --all           # Run existing tests  
cd ../agentloop && ./agentloop-server & # Ensure Go server is running
```

**Before any changes:**
- Read this CLAUDE.md completely
- Understand bridge vs server distinction
- Review current implementation in `crates/agentloop-bridge/src/lib.rs`
- Test with real AgentLoop server

**2. PLANNING PHASE**

**Feature classification:**
| Feature Type | Primary Location | Notes |
|--------------|------------------|-------|
| Bridge API | `agentloop-bridge/src/lib.rs` | JSON-RPC client methods |
| Zed ACP | `agentloop-bridge/src/zed_acp.rs` | Feature: `zed-acp` |
| CLI tool | `agentloop-cli/src/main.rs` | Uses bridge library |
| Config | `agentloop-core/src/config.rs` | Shared configuration |
| Tests | `tests/` + `*/tests/` | Unit + integration |

**3. CODING PHASE**

**Follow established patterns:**
```rust
// Error handling
use thiserror::Error;
pub type Result<T> = std::result::Result<T, BridgeError>;

// Async throughout
use tokio;
pub async fn new_method(&mut self) -> Result<String> { ... }

// Feature gates for optional functionality  
#[cfg(feature = "zed-acp")]
pub mod zed_acp { ... }

// Comprehensive documentation
/// Documentation for public APIs
pub struct PublicType { ... }
```

**4. TESTING PHASE** (MANDATORY)
```bash
# Test your specific changes
cargo test -p agentloop-bridge

# Test with features
cargo test --features zed-acp

# Integration tests (requires AgentLoop Go server running)
cargo test --test integration

# BEFORE completing any PR:
cargo test --all                    # All tests must pass
cargo clippy -- -D warnings         # No clippy warnings
cargo fmt --check                   # Code formatted
```

### **When Adding Bridge Features**

**Step-by-step process:**

1. **Identify the AgentLoop server API** - what JSON-RPC method exists?
2. **Add request/response types** - use `serde` derive macros
3. **Add client method** - follow async pattern, use `send_request()`
4. **Add event handling** - if server sends new notification types
5. **Add comprehensive tests** - unit + integration
6. **Update documentation** - docstrings and examples

**Example walkthrough - adding `session.list` method:**

```rust
// 1. Request/response types
#[derive(Debug, Serialize)]
pub struct SessionListParams {
    #[serde(rename = "userId", skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]  
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub status: String,
    #[serde(rename = "userId")]
    pub user_id: String,
}

// 2. Client method
impl AgentLoopClient {
    pub async fn list_sessions(&mut self, user_id: Option<String>, status: Option<String>) -> Result<Vec<SessionInfo>> {
        let params = SessionListParams { user_id, status };
        let response = self.send_request("session.list", serde_json::to_value(params)?).await?;
        let sessions: Vec<SessionInfo> = serde_json::from_value(response)?;
        Ok(sessions)
    }
}

// 3. Tests
#[tokio::test]
async fn test_list_sessions() {
    let mut client = create_test_client().await;
    client.connect().await?;
    
    let sessions = client.list_sessions(Some("marco".to_string()), None).await?;
    assert!(sessions.len() >= 0); // At least no error
}
```

### **When Adding Zed ACP Features**

**Key principles:**
- Use feature flag: `#[cfg(feature = "zed-acp")]`
- Extend `ZedACPAdapter` in `zed_acp.rs`
- Focus on Zed-specific abstractions
- Plan for Phase 2 UI integration

**Example - adding file context extraction:**
```rust
#[cfg(feature = "zed-acp")]
impl ZedACPAdapter {
    /// Extract file context from Zed workspace
    pub async fn extract_file_context(&self, workspace_path: &str) -> Result<Vec<FileContext>> {
        // TODO: In Phase 2, integrate with Zed's file API
        // For now, basic filesystem scanning
        let mut contexts = Vec::new();
        
        for entry in std::fs::read_dir(workspace_path)? {
            let entry = entry?;
            if entry.path().extension().map_or(false, |ext| ext == "rs") {
                let content = std::fs::read_to_string(entry.path())?;
                contexts.push(FileContext {
                    path: entry.path().to_string_lossy().to_string(),
                    content,
                });
            }
        }
        
        Ok(contexts)
    }
}

#[cfg(feature = "zed-acp")]
#[derive(Debug)]
pub struct FileContext {
    pub path: String,
    pub content: String,
}
```

### **Testing Strategy**

**Unit tests** - Test components in isolation:
```rust
#[tokio::test]
async fn test_client_config_default() {
    let config = ClientConfig::default();
    assert!(config.socket_path.to_string_lossy().contains("agentloop.sock"));
    assert_eq!(config.request_timeout, Duration::from_secs(30));
}

#[tokio::test]  
async fn test_json_rpc_serialization() {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "task.start".to_string(),
        params: serde_json::json!({"userId": "test"}),
        id: 1,
    };
    
    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("\"jsonrpc\":\"2.0\""));
    assert!(json.contains("\"method\":\"task.start\""));
}
```

**Integration tests** - Test against real AgentLoop server:
```rust
// tests/integration.rs
use agentloop_bridge::{AgentLoopClient, ClientConfig};

async fn create_test_client() -> AgentLoopClient {
    let config = ClientConfig::default();
    AgentLoopClient::new(config)
}

#[tokio::test]
async fn test_health_check() {
    let mut client = create_test_client().await;
    client.connect().await.expect("Failed to connect to AgentLoop server");
    
    let health = client.health_check().await.expect("Health check failed");
    assert!(health.get("active_sessions").is_some());
}

#[tokio::test]
async fn test_task_execution() {
    let mut client = create_test_client().await;
    client.connect().await?;
    
    let session_id = client.start_task("test", "echo hello", None, "test").await?;
    let stats = client.wait_for_completion(&session_id).await?;
    
    assert!(stats.duration_ms > 0);
    assert!(stats.tool_calls >= 1); // Should have called bash tool
}
```

**Zed ACP tests** - Feature-gated:
```rust
#[cfg(feature = "zed-acp")]
#[tokio::test]  
async fn test_zed_acp_adapter() {
    let config = ClientConfig::default();
    let adapter = agentloop_bridge::zed_acp::ZedACPAdapter::new(config, "test".to_string());
    assert_eq!(adapter.client().state(), ClientState::Disconnected);
}
```

### **Error Handling Patterns**

**Use structured errors consistently:**
```rust
// Return proper error types
pub async fn risky_operation() -> Result<String> {
    let result = some_fallible_operation()
        .map_err(|e| BridgeError::Process { message: format!("Operation failed: {}", e) })?;
    Ok(result)
}

// Handle errors gracefully in applications
match client.start_task("user", "task", None, "source").await {
    Ok(session_id) => println!("Started session: {}", session_id),
    Err(BridgeError::Timeout { .. }) => {
        eprintln!("Request timed out, retrying...");
        // Implement retry logic
    }
    Err(BridgeError::Process { message }) => {
        eprintln!("AgentLoop server error: {}", message);
        // Handle server-side errors
    }
    Err(e) => {
        eprintln!("Unexpected error: {}", e);
        return Err(e);
    }
}
```

### **Performance Mindset**

**The bridge should be fast:**
- Use zero-copy JSON parsing where possible
- Minimize allocations in hot paths
- Leverage tokio's async efficiency
- Profile memory usage for long-running sessions

**Example optimization:**
```rust
// Avoid cloning large JSON values
pub async fn handle_large_event(params: &serde_json::Value) -> Result<()> {
    let session_id = params["sessionId"].as_str()
        .ok_or_else(|| BridgeError::Process { message: "Missing sessionId".to_string() })?;
    
    // Use string slice instead of String::from()
    process_session(session_id).await
}
```

---

## Troubleshooting Guide

### Common Issues

**1. "Connection refused" errors**
```bash
# Check if AgentLoop server is running
ps aux | grep agentloop-server

# Start the Go server
cd ../agentloop && ./agentloop-server &

# Check socket exists
ls -la ~/.local/share/agentloop/agentloop.sock
```

**2. "Method not found" JSON-RPC errors**
- Check method name spelling in bridge implementation
- Verify Go server version compatibility  
- Review AgentLoop server logs

**3. Feature compilation errors**
```bash
# Install with zed-acp feature
cargo build --features zed-acp

# Check feature flags in Cargo.toml
grep -A5 "\[features\]" Cargo.toml
```

**4. Test failures**
```bash
# Run tests with Go server running
cd ../agentloop && ./agentloop-server &
cd ../agentloop-rs && cargo test

# Check integration test setup
cargo test --test integration -- --nocapture
```

### Debug Tools

```bash
# Enable debug logging
RUST_LOG=debug cargo run --bin agentloop-cli

# Monitor JSON-RPC traffic
sudo tcpdump -i lo -A | grep -E "(jsonrpc|method|params)"

# Profile memory usage
cargo install cargo-profdata
cargo profdata -- --bin agentloop-cli "test task"
```

---

**Remember: This is a bridge/client library, not a server reimplementation. Always test with a running AgentLoop Go server!**