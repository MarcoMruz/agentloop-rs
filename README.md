# AgentLoop Rust Bridge

🚀 **Rust client library and Zed ACP integration for AgentLoop**

## Overview

AgentLoop-rs provides a **bridge client library** for connecting applications to the AgentLoop server via Unix socket JSON-RPC protocol. The primary use case is **Zed editor integration** through Adaptive Code Provider (ACP) infrastructure.

**This is NOT a server reimplementation** - it's a client that communicates with the existing Go AgentLoop server.

## Architecture

```
┌─────────────┐    ┌──────────────────┐    ┌─────────────────┐
│ Zed Editor  │────│ agentloop-bridge │────│ AgentLoop       │
│             │    │ (Rust client)    │    │ Server (Go)     │
│ - ACP       │    │                  │    │                 │
│ - LSP       │    │ - JSON-RPC 2.0   │    │ - Memory mgmt   │
│ - UI        │    │ - Unix socket    │    │ - Session mgmt  │
└─────────────┘    │ - Event streams  │    │ - HITL approval │
                   └──────────────────┘    │ - Agent core    │
                                           └─────────────────┘
```

## Project Structure

```
agentloop-rs/
├── crates/
│   ├── agentloop-bridge/   # 🎯 CORE: Client library for AgentLoop server
│   ├── agentloop-cli/      # CLI tool using the bridge
│   ├── agentloop-core/     # Shared types and utilities
│   └── agentloop-server/   # [Future] Optional Rust server implementation
├── examples/               # Usage examples
└── src/                    # Workspace root
```

## Key Features

✅ **AgentLoop Bridge (COMPLETED)**
- [x] JSON-RPC 2.0 client over Unix socket
- [x] Full API compatibility with AgentLoop Go server
- [x] Event streaming (text, tool_use, tool_result, hitl_request, done, error)
- [x] Session management (start, steer, abort, wait)
- [x] HITL approval workflow
- [x] Async/await with Tokio
- [x] Thread-safe design with proper error handling

✅ **Zed ACP Integration (PARTIAL)**
- [x] Basic ZedACPAdapter structure
- [x] Feature flag: `zed-acp`
- [x] Coding task execution interface
- [ ] Full Zed UI integration (Phase 2)
- [ ] Interactive HITL approval in Zed (Phase 2)
- [ ] File context extraction (Phase 2)

📋 **CLI Tool (BASIC)**
- [x] Task execution via command line
- [x] Health checks
- [ ] Interactive session management
- [ ] Session history

🔮 **Optional Rust Server (FUTURE)**
- [ ] Alternative to Go server
- [ ] API-compatible implementation
- [ ] Performance optimizations

## Quick Start

### Prerequisites

```bash
# Required: AgentLoop Go server
git clone <agentloop-main-repo>
cd agentloop && ./agentloop-server &

# Required: pi coding agent
npm install -g @mariozechner/pi-coding-agent

# Required: Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustc --version  # Should be 1.70+
```

### Build & Test

```bash
git clone <this-repo>
cd agentloop-rs

# Build all crates
cargo build --release

# Test bridge functionality
cargo test -p agentloop-bridge

# Test with Zed ACP features
cargo test --features zed-acp

# Build CLI tool
cargo build --bin agentloop-cli
```

### Usage

**1. As a Library (Primary Use Case)**

```rust
use agentloop_bridge::{AgentLoopClient, ClientConfig, HITLDecision};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ClientConfig::default();
    let mut client = AgentLoopClient::new(config);
    
    // Connect to running AgentLoop server
    client.connect().await?;
    
    // Start a coding task
    let session_id = client.start_task(
        "marco",                    // user_id
        "Fix the failing tests",    // task description  
        Some("/home/marco/project"),// work_dir
        "zed"                       // source
    ).await?;
    
    // Handle events
    let mut event_rx = client.take_event_receiver().unwrap();
    while let Some(event) = event_rx.recv().await {
        match event {
            AgentEvent::Text { content, .. } => {
                print!("{}", content);
            }
            AgentEvent::HITLRequest { session_id, request_id, tool_name, details, .. } => {
                // Auto-approve safe operations
                let decision = if tool_name == "read" { 
                    HITLDecision::Approve 
                } else { 
                    HITLDecision::Deny 
                };
                client.respond_hitl(&session_id, &request_id, decision).await?;
            }
            AgentEvent::Done { output, stats, .. } => {
                println!("✅ Task completed: {}", output);
                println!("📊 {} tool calls, {} HITL requests", stats.tool_calls, stats.hitl_requests);
                break;
            }
            AgentEvent::Error { message, .. } => {
                eprintln!("❌ Error: {}", message);
                break;
            }
            _ => {}
        }
    }
    
    Ok(())
}
```

**2. With Zed ACP (Future)**

```rust
use agentloop_bridge::{ClientConfig, zed_acp::ZedACPAdapter};

#[cfg(feature = "zed-acp")]
async fn zed_integration() -> Result<(), Box<dyn std::error::Error>> {
    let config = ClientConfig::default();
    let mut adapter = ZedACPAdapter::new(config, "marco".to_string());
    
    let result = adapter.execute_coding_task(
        "Refactor this function for better performance",
        Some("/home/marco/project")
    ).await?;
    
    println!("Zed ACP result: {}", result);
    Ok(())
}
```

**3. CLI Usage**

```bash
# Install CLI tool
cargo install --path crates/agentloop-cli

# Execute a task (requires running AgentLoop server)
agentloop-cli "analyze the codebase and suggest improvements"

# Check server health
agentloop-cli --health

# Interactive mode
agentloop-cli --interactive
```

### Configuration

Place `agentloop.yaml` in `~/.config/agentloop/`:

```yaml
# Bridge client config
client:
  socket_path: ~/.local/share/agentloop/agentloop.sock
  request_timeout: 30s
  max_retries: 3
  retry_delay: 1s
  event_buffer_size: 1000

# Zed ACP config (when feature enabled)
zed:
  user_id: marco
  workspace_detection: true
  auto_approve_safe_tools: true
  interactive_hitl: true
```

## Development

### Running Tests

```bash
# All tests
cargo test --all

# Bridge tests only
cargo test -p agentloop-bridge

# With Zed ACP features
cargo test --features zed-acp

# Integration tests (requires running AgentLoop server)
cargo test --test integration
```

### Adding New Features

1. **Bridge features**: Add to `crates/agentloop-bridge/src/lib.rs`
2. **CLI features**: Add to `crates/agentloop-cli/src/main.rs`  
3. **Zed features**: Add to `crates/agentloop-bridge/src/zed_acp.rs`
4. **Tests**: Add corresponding test cases

### Code Quality

```bash
# Format
cargo fmt

# Linting
cargo clippy -- -D warnings

# Documentation
cargo doc --no-deps --open

# Security audit
cargo audit
```

## JSON-RPC API Compatibility

The bridge implements 100% compatibility with AgentLoop Go server:

| Method | Parameters | Description |
|--------|------------|-------------|
| `task.start` | `{userId, text, workDir?, source}` | Start new task |
| `task.steer` | `{sessionId, text}` | Redirect task |
| `task.abort` | `{sessionId}` | Abort task |
| `hitl.respond` | `{sessionId, requestId, decision}` | HITL approval |
| `health.check` | `{}` | Server health |

**Events (Server → Client):**
- `event.text` - Streaming output
- `event.tool_use` - Tool execution start  
- `event.tool_result` - Tool execution result
- `event.hitl_request` - Human approval needed
- `event.done` - Task completed
- `event.error` - Task failed

## Deployment

### Development
```bash
# Run AgentLoop server (Go)
cd ../agentloop && ./agentloop-server &

# Use Rust bridge/CLI
cd agentloop-rs && cargo run --bin agentloop-cli -- "test task"
```

### Production
```bash
# Install bridge as library dependency
cargo add agentloop-bridge

# Or install CLI globally
cargo install --git <repo-url> agentloop-cli
```

### Zed Editor Integration
```bash
# Build with Zed ACP support
cargo build --features zed-acp --release

# Integration with Zed extension system (Phase 2)
# See: docs/zed-integration.md
```

## Roadmap

### 🎯 Phase 1: Core Bridge (COMPLETED)
- [x] JSON-RPC 2.0 client implementation
- [x] Unix socket communication
- [x] Event handling and streaming
- [x] Session lifecycle management
- [x] HITL workflow support
- [x] Error handling and reconnection
- [x] Basic CLI tool

### 🔄 Phase 2: Zed Integration (IN PROGRESS)
- [x] ZedACPAdapter basic structure
- [ ] Interactive HITL approval in Zed UI
- [ ] File context extraction from Zed
- [ ] Symbol-aware task prompting
- [ ] Real-time code updates via ACP
- [ ] Error diagnostics integration
- [ ] Zed extension packaging

### 📋 Phase 3: Advanced Features (PLANNED)
- [ ] Connection pooling and load balancing
- [ ] Offline task queuing
- [ ] Performance metrics and monitoring
- [ ] Advanced configuration management
- [ ] Plugin system for custom integrations
- [ ] Multi-server support

### 🔮 Phase 4: Optional Server (FUTURE)
- [ ] Rust server implementation (API-compatible)
- [ ] Performance optimizations
- [ ] Memory usage improvements
- [ ] Advanced concurrency patterns
- [ ] Cloud deployment optimizations

## Contributing

1. Follow Rust best practices (`cargo fmt`, `cargo clippy`)
2. Add tests for new functionality
3. Update documentation
4. Maintain API compatibility with AgentLoop Go server
5. Use feature flags for optional functionality

### Security

Since this is a client library, security focuses on:
- Input validation for JSON-RPC requests
- Secure socket communication
- Memory safety (Rust guarantees)
- No unsafe code allowed
- Dependency auditing

## FAQ

**Q: Why Rust for the bridge?**
A: Memory safety, performance, and excellent async ecosystem for handling concurrent sessions and events.

**Q: Can I use this without the Go server?**
A: Currently no - the bridge requires a running AgentLoop server. A Rust server implementation is planned for Phase 4.

**Q: Is the Zed integration ready?**
A: Basic structure is ready, full integration comes in Phase 2. Currently you can use the bridge manually.

**Q: Performance compared to Go client?**
A: Rust bridge should be faster due to zero-copy JSON parsing and efficient async handling, but both are fast enough for interactive use.

**Q: Backwards compatibility?**
A: 100% API compatible with AgentLoop Go server. Bridge can be dropped-in replacement for any JSON-RPC client.

## License

Same as the main AgentLoop project.