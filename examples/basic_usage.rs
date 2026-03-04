//! Basic usage example for AgentLoop Rust bridge
//!
//! This example demonstrates:
//! - Connecting to AgentLoop server
//! - Starting a simple task
//! - Handling events
//! - Waiting for completion

use agentloop_bridge::{AgentEvent, AgentLoopClient, ClientConfig, HITLDecision};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("🚀 AgentLoop Rust Bridge - Basic Usage Example");
    
    // Create client with default configuration
    let config = ClientConfig::default();
    let mut client = AgentLoopClient::new(config);
    
    // Get event receiver before connecting
    let mut event_rx = client.take_event_receiver()
        .expect("Failed to get event receiver");
    
    println!("📡 Connecting to AgentLoop server...");
    
    // Connect to server with retry logic
    for attempt in 1..=3 {
        match client.connect().await {
            Ok(_) => {
                println!("✅ Connected to AgentLoop server!");
                break;
            }
            Err(e) => {
                eprintln!("❌ Connection attempt {} failed: {}", attempt, e);
                if attempt == 3 {
                    eprintln!("💀 Failed to connect after 3 attempts. Is AgentLoop server running?");
                    eprintln!("   Start it with: cd ../agentloop && ./agentloop-server &");
                    return Err(e.into());
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    
    // Check server health
    match client.health_check().await {
        Ok(health) => println!("🏥 Server health: {:?}", health),
        Err(e) => eprintln!("⚠️  Health check failed: {}", e),
    }
    
    // Start a simple task
    let task = "Analyze the current directory structure and create a brief summary";
    let work_dir = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(String::from));
    
    println!("🎯 Starting task: {}", task);
    println!("📁 Work directory: {:?}", work_dir);
    
    let session_id = client.start_task("example_user", task, work_dir, "rust_bridge_example").await?;
    println!("🆔 Session ID: {}", session_id);
    
    // Handle events until completion
    println!("👂 Listening for events...\n");
    
    let mut tool_calls = 0;
    let mut hitl_requests = 0;
    
    while let Some(event) = event_rx.recv().await {
        match event {
            AgentEvent::StateChanged(state) => {
                println!("🔄 Connection state changed: {:?}", state);
            }
            AgentEvent::Text { session_id: _, content } => {
                // Stream text output
                print!("{}", content);
            }
            AgentEvent::ToolUse { session_id: _, tool_name, input } => {
                tool_calls += 1;
                println!("\n🔧 Tool #{}: {} with input: {:?}", tool_calls, tool_name, input);
            }
            AgentEvent::ToolResult { session_id: _, tool_name, output, success } => {
                let status = if success { "✅" } else { "❌" };
                println!("{} Tool {} result: {}", status, tool_name, output.lines().take(3).collect::<Vec<_>>().join("\n"));
                if output.lines().count() > 3 {
                    println!("   ... (output truncated)");
                }
            }
            AgentEvent::HITLRequest { 
                session_id, 
                request_id, 
                tool_name, 
                details, 
                options 
            } => {
                hitl_requests += 1;
                println!("\n🤔 HITL Request #{}: {} needs approval", hitl_requests, tool_name);
                println!("   Details: {}", details);
                println!("   Options: {:?}", options);
                
                // Auto-approve safe operations for demo
                let decision = match tool_name.as_str() {
                    "read" | "grep" | "find" | "ls" => {
                        println!("   ✅ Auto-approving safe operation");
                        HITLDecision::Approve
                    }
                    "bash" => {
                        // Check if it's a safe bash command
                        if details.contains("ls") || details.contains("pwd") || details.contains("find") {
                            println!("   ✅ Auto-approving safe bash command");
                            HITLDecision::Approve
                        } else {
                            println!("   ⚠️  Denying potentially unsafe bash command");
                            HITLDecision::Deny
                        }
                    }
                    "edit" | "write" => {
                        println!("   ❌ Denying write operation for safety");
                        HITLDecision::Deny
                    }
                    _ => {
                        println!("   ❓ Unknown tool, denying for safety");
                        HITLDecision::Deny
                    }
                };
                
                // Send decision
                if let Err(e) = client.respond_hitl(&session_id, &request_id, decision).await {
                    eprintln!("❌ Failed to respond to HITL: {}", e);
                }
            }
            AgentEvent::Done { session_id: _, output, stats } => {
                println!("\n\n🎉 Task completed successfully!");
                println!("📄 Final output: {}", output);
                println!("📊 Statistics:");
                println!("   Duration: {}ms", stats.duration_ms);
                println!("   Tool calls: {}", stats.tool_calls);
                println!("   HITL requests: {}", stats.hitl_requests);
                if let Some(tokens) = stats.tokens_used {
                    println!("   Tokens used: {}", tokens);
                }
                break;
            }
            AgentEvent::Error { session_id: _, message } => {
                eprintln!("\n❌ Task failed: {}", message);
                break;
            }
            AgentEvent::SessionSaved { session_id: _ } => {
                println!("\n💾 Session saved to vault");
            }
        }
    }
    
    // Disconnect gracefully
    println!("\n👋 Disconnecting from server...");
    client.disconnect().await?;
    println!("✅ Disconnected successfully!");
    
    Ok(())
}