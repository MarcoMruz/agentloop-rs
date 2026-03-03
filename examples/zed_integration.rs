//! Example of using agentloop-bridge for Zed editor integration
//!
//! This example demonstrates how to use the AgentLoop client to connect
//! to the AgentLoop server and execute coding tasks, similar to how
//! agentloop-slack bridges Slack to AgentLoop.
//!
//! Usage:
//! ```bash
//! # First, start the AgentLoop server (Go or Rust version)
//! cd ../agentloop && ./agentloop-server &
//!
//! # Then run this example
//! cargo run --example zed_integration --features zed-acp
//! ```

#[cfg(feature = "zed-acp")]
use agentloop_bridge::{ClientConfig, zed_acp::ZedACPAdapter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    #[cfg(feature = "zed-acp")]
    {
        // Create configuration pointing to AgentLoop server socket
        let config = ClientConfig::default();
        
        // Create Zed ACP adapter with current user context
        let mut adapter = ZedACPAdapter::new(config, "marco".to_string());

        println!("🔗 Connecting to AgentLoop server...");
        
        // Execute a coding task (simulating Zed editor context)
        let result = adapter.execute_coding_task(
            "Analyze the Rust code in this workspace and suggest improvements for memory safety",
            Some("/home/marco/dev/agentloop-rs")
        ).await?;

        println!("✅ Task completed: {}", result);

        // Demonstrate event handling (in a real Zed integration, this would be more sophisticated)
        if let Some(mut event_rx) = adapter.client_mut().take_event_receiver() {
            println!("📡 Listening for server events...");
            
            // In a real Zed integration, these events would update the editor UI
            tokio::spawn(async move {
                while let Some(event) = event_rx.recv().await {
                    match event {
                        agentloop_bridge::AgentEvent::Text { session_id, content } => {
                            println!("📝 [{session_id}] Text: {content}");
                        }
                        agentloop_bridge::AgentEvent::ToolUse { session_id, tool_name, .. } => {
                            println!("🔧 [{session_id}] Using tool: {tool_name}");
                        }
                        agentloop_bridge::AgentEvent::HITLRequest { session_id, tool_name, details, .. } => {
                            println!("❓ [{session_id}] HITL approval needed for {tool_name}: {details}");
                            // In real Zed integration, this would show an approval dialog
                        }
                        agentloop_bridge::AgentEvent::Done { session_id, .. } => {
                            println!("✅ [{session_id}] Task completed!");
                            break;
                        }
                        agentloop_bridge::AgentEvent::Error { session_id, message } => {
                            println!("❌ [{session_id}] Error: {message}");
                            break;
                        }
                        _ => {} // Handle other event types as needed
                    }
                }
            });
        }
    }

    #[cfg(not(feature = "zed-acp"))]
    {
        println!("❌ This example requires the 'zed-acp' feature. Run with:");
        println!("   cargo run --example zed_integration --features zed-acp");
    }

    Ok(())
}