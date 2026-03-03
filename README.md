# AgentLoop Rust Implementation

🚀 **High-performance, memory-safe reimplementation of AgentLoop in Rust**

## Overview

This is a complete reimplementation of AgentLoop in Rust, designed for production use with enhanced performance, memory safety, and modularity. The Rust version maintains full compatibility with the original Go version's JSON-RPC API while providing improved resource management and standalone components.

## Architecture

```
agentloop-rs/
├── crates/
│   ├── agentloop-core/     # Core business logic
│   ├── agentloop-server/   # Server binary
│   ├── agentloop-cli/      # CLI client
│   └── agentloop-bridge/   # Standalone pi bridge
└── src/                    # Workspace utilities
```

## Features

✅ **Phase 1 (Completed):**
- [x] Core Rust infrastructure
- [x] UNIX socket server with Tokio
- [x] JSON-RPC 2.0 protocol handling
- [x] Basic session management
- [x] Bridge API design
- [x] Configuration system

✅ **Phase 2 (Completed):**
- [x] Bridge implementation (now AgentLoop server client)
- [x] JSON-RPC 2.0 protocol with Unix socket
- [x] Event handling and streaming
- [x] Session management integration

📋 **Phase 3 (Planned):**
- [ ] Memory engine
- [ ] Vault persistence
- [ ] Skills system
- [ ] Security validation

🔮 **Phase 4 (Future):**
- [ ] Zed ACP integration
- [ ] Performance optimizations
- [ ] Advanced features

## Quick Start

### Prerequisites

- Rust 1.70+ (`rustup` recommended)
- `pi` coding agent v0.54.0+: `npm install -g @mariozechner/pi-coding-agent`

### Build

```bash
# Clone and build
git clone <repository>
cd agentloop-rs
cargo build --release

# Build specific components
cargo build --bin agentloop-server
cargo build --bin agentloop
```

### Run

**Option 1: Use Go AgentLoop server (recommended for Zed integration)**
```bash
# Start the Go AgentLoop server (in agentloop repo)
cd ../agentloop
./agentloop-server &

# Use Rust client to communicate with Go server
cd ../agentloop-rs
cargo run --bin agentloop -- "describe this project structure"
```

**Option 2: Use Rust AgentLoop server (standalone)**
```bash
# Start the Rust server
cargo run --bin agentloop-server

# In another terminal, use the CLI
cargo run --bin agentloop -- "describe this project structure"

# Or install globally
cargo install --path crates/agentloop-server
cargo install --path crates/agentloop-cli
```

### Configuration

Place `agentloop.yaml` in `~/.config/agentloop/`:

```yaml
server:
  socket_path: ~/.local/share/agentloop/agentloop.sock
pi:
  binary_path: pi
  provider: anthropic
  model: claude-sonnet-4-20250514
# ... see agentloop.yaml for full config
```

## Crate Documentation

### agentloop-core

Core library containing:
- **Server**: UNIX socket server with JSON-RPC 2.0
- **Session**: Session management and lifecycle
- **Config**: Configuration loading with YAML + env vars
- **Errors**: Structured error taxonomy

### agentloop-bridge

Client library for AgentLoop server communication:
- **Communicates with AgentLoop server via Unix socket** (like agentloop-slack)
- **Gets memory management, HITL, session management from server**
- JSON-RPC 2.0 protocol with event streaming
- Supports Zed ACP integration (feature: `zed-acp`)
- Can connect to Go or Rust AgentLoop server implementations

### agentloop-server

Long-running server binary:
- Manages sessions, memory, and agent execution
- UNIX socket API identical to Go version
- Graceful shutdown handling

### agentloop-cli

Command-line client:
- Connects to server via UNIX socket
- Interactive task execution
- Session management commands
- Health checking

## API Compatibility

The Rust implementation maintains 100% JSON-RPC API compatibility with the Go version:

```json
// Start a task
{"jsonrpc":"2.0","method":"task.start","params":{"userId":"user","text":"fix tests","source":"cli"},"id":1}

// Events (notifications)
{"jsonrpc":"2.0","method":"event.text","params":{"sessionId":"sess-12345","content":"Hello"}}
{"jsonrpc":"2.0","method":"event.done","params":{"sessionId":"sess-12345","output":"Done","stats":{}}}
```

## Development

### Testing

```bash
# Run all tests
cargo test

# Test specific crate
cargo test -p agentloop-core

# Test with all features
cargo test --all-features
```

### Linting

```bash
# Format code
cargo fmt

# Run clippy
cargo clippy

# Check documentation
cargo doc --no-deps --open
```

### Features

- `zed-acp`: Enable Zed ACP integration in agentloop-bridge

## Migration from Go Version

The Rust version can run alongside the Go version without conflicts:

1. **Bridge as client**: agentloop-bridge connects to existing Go server
2. **Same vault format**: Both versions use identical Markdown+YAML vault format  
3. **API compatible**: All JSON-RPC methods are identical
4. **Zed integration**: Use agentloop-bridge to connect Zed to Go server
5. **Gradual migration**: Can migrate server to Rust later while keeping clients working

## Performance Benefits

- **Memory safety**: No buffer overflows or memory leaks
- **Async efficiency**: Tokio-based async runtime
- **Zero-copy parsing**: Efficient JSON handling
- **Resource management**: Automatic cleanup via RAII
- **Concurrent safety**: Fearless concurrency with Send/Sync

## Contributing

1. Follow Rust conventions (`cargo fmt`, `cargo clippy`)
2. Add tests for new functionality
3. Update documentation
4. Maintain API compatibility
5. Security changes require explicit approval

## License

Same as the original AgentLoop project.