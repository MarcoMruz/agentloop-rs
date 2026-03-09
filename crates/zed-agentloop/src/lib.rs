//! Zed editor extension for AgentLoop ACP integration.
//!
//! AgentLoop integrates with Zed as a first-class **ACP agent** (Agent Client
//! Protocol), appearing in the Zed Agent panel alongside Claude Code, Gemini
//! CLI, and Codex.
//!
//! # Architecture
//!
//! ```text
//! Zed Editor (Agent panel)
//!   └─ spawns: agentloop-acp-bridge  ← native ACP binary
//!        ├─ ACP (JSON-RPC 2.0) over stdin/stdout  ← talks to Zed
//!        └─ JSON-RPC 2.0 over Unix socket          ← talks to AgentLoop Go server
//!             └─ AgentLoop Go server
//! ```
//!
//! # Installation & Configuration
//!
//! 1. Install the ACP bridge binary:
//!    ```sh
//!    cargo install --path crates/agentloop-acp-bridge
//!    # or: ./install.sh
//!    ```
//!
//! 2. Add AgentLoop as a custom ACP agent in `~/.config/zed/settings.json`:
//!    ```json
//!    {
//!      "agent_servers": {
//!        "AgentLoop": {
//!          "type": "custom",
//!          "command": "agentloop-acp-bridge",
//!          "args": ["--socket-path", "~/.local/share/agentloop/agentloop.sock"],
//!          "env": {}
//!        }
//!      }
//!    }
//!    ```
//!
//! 3. Ensure the AgentLoop Go server is running:
//!    ```sh
//!    agentloop-server &
//!    ```
//!
//! # Extension API Note
//!
//! The `zed_extension_api` crate (v0.7.0) exposes `context_server_command` for
//! MCP context servers but does not yet include an `agent_server_command` method
//! for ACP agents.  When that API becomes available this extension will be
//! updated to register AgentLoop automatically — without requiring manual
//! settings.json configuration.

use zed_extension_api::{self as zed};

struct AgentLoopExtension;

impl zed::Extension for AgentLoopExtension {
    fn new() -> Self {
        Self
    }
}

zed::register_extension!(AgentLoopExtension);
