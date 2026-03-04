//! Zed ACP integration example for AgentLoop Rust bridge
//!
//! This example demonstrates:
//! - ZedACPAdapter usage
//! - File context extraction
//! - Interactive HITL approval simulation
//! - Zed-specific features
//!
//! Note: This example requires the `zed-acp` feature to be enabled:
//! ```bash
//! cargo run --example zed_integration --features zed-acp
//! ```

#[cfg(not(feature = "zed-acp"))]
fn main() {
    eprintln!("❌ This example requires the 'zed-acp' feature.");
    eprintln!("Run with: cargo run --example zed_integration --features zed-acp");
    std::process::exit(1);
}

#[cfg(feature = "zed-acp")]
use agentloop_bridge::{
    zed_acp::ZedACPAdapter,
    AgentEvent, ClientConfig, HITLDecision
};

#[cfg(feature = "zed-acp")]
use std::path::PathBuf;

#[cfg(feature = "zed-acp")]
#[derive(Debug)]
struct MockZedWorkspace {
    root_path: PathBuf,
    open_files: Vec<String>,
    current_selection: Option<String>,
}

#[cfg(feature = "zed-acp")]
impl MockZedWorkspace {
    fn new(root_path: impl Into<PathBuf>) -> Self {
        Self {
            root_path: root_path.into(),
            open_files: Vec::new(),
            current_selection: None,
        }
    }
    
    fn add_open_file(&mut self, path: String) {
        self.open_files.push(path);
    }
    
    fn set_selection(&mut self, selection: String) {
        self.current_selection = Some(selection);
    }
    
    fn get_context_prompt(&self) -> String {
        let mut prompt = format!("Working in project: {}\n", self.root_path.display());
        
        if !self.open_files.is_empty() {
            prompt.push_str("Currently open files:\n");
            for file in &self.open_files {
                prompt.push_str(&format!("- {}\n", file));
            }
        }
        
        if let Some(selection) = &self.current_selection {
            prompt.push_str(&format!("Selected code:\n```\n{}\n```\n", selection));
        }
        
        prompt
    }
}

#[cfg(feature = "zed-acp")]
async fn simulate_zed_coding_session() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎯 Simulating Zed coding session with AgentLoop...");
    
    // Mock Zed workspace setup
    let mut workspace = MockZedWorkspace::new(std::env::current_dir()?);
    workspace.add_open_file("src/lib.rs".to_string());
    workspace.add_open_file("examples/zed_integration.rs".to_string());
    workspace.set_selection("fn main() {\n    println!(\"Hello, world!\");\n}".to_string());
    
    println!("📁 Workspace: {}", workspace.root_path.display());
    println!("📝 Open files: {:?}", workspace.open_files);
    
    // Create Zed ACP adapter
    let config = ClientConfig::default();
    let mut adapter = ZedACPAdapter::new(config, "zed_user".to_string());
    
    // Connect to AgentLoop server
    println!("📡 Connecting to AgentLoop via ZedACPAdapter...");
    adapter.client_mut().connect().await?;
    println!("✅ Connected!");
    
    // Simulate various Zed ACP scenarios
    let scenarios = vec![
        (
            "Code Review",
            format!("{}Review this code and suggest improvements for performance and readability.", 
                   workspace.get_context_prompt())
        ),
        (
            "Add Tests",
            format!("{}Generate comprehensive unit tests for the selected function.", 
                   workspace.get_context_prompt())
        ),
        (
            "Refactor",
            format!("{}Refactor this code to use more idiomatic Rust patterns.", 
                   workspace.get_context_prompt())
        ),
        (
            "Documentation",
            format!("{}Add detailed documentation comments to this code.", 
                   workspace.get_context_prompt())
        ),
    ];
    
    for (scenario_name, prompt) in scenarios {
        println!("\n🎭 Scenario: {}", scenario_name);
        println!("💭 Prompt length: {} chars", prompt.len());
        
        // Execute coding task
        match adapter.execute_coding_task(&prompt, Some(workspace.root_path.to_str().unwrap())).await {
            Ok(result) => {
                println!("✅ {}: {}", scenario_name, result);
            }
            Err(e) => {
                eprintln!("❌ {} failed: {}", scenario_name, e);
            }
        }
        
        // Small delay between scenarios
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    
    // Disconnect
    println!("\n👋 Disconnecting from AgentLoop...");
    adapter.client_mut().disconnect().await?;
    
    Ok(())
}

#[cfg(feature = "zed-acp")]
async fn interactive_hitl_demo() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🤔 Interactive HITL Demo");
    
    let config = ClientConfig::default();
    let mut adapter = ZedACPAdapter::new(config, "hitl_demo_user".to_string());
    
    // Connect
    adapter.client_mut().connect().await?;
    
    // Start a task that will trigger HITL
    let session_id = adapter.client_mut().start_task(
        "hitl_demo_user",
        "Run 'ls -la' to see directory contents, then create a test file",
        Some(std::env::current_dir()?.to_string_lossy().to_string()),
        "zed-hitl-demo"
    ).await?;
    
    println!("🆔 Started HITL demo session: {}", session_id);
    
    // Get event receiver and handle HITL requests
    let mut event_rx = adapter.client_mut().take_event_receiver().unwrap();
    
    while let Some(event) = event_rx.recv().await {
        match event {
            AgentEvent::Text { content, .. } => {
                print!("{}", content);
            }
            AgentEvent::HITLRequest { session_id, request_id, tool_name, details, options } => {
                println!("\n🤔 HITL Request from Zed context:");
                println!("   Tool: {}", tool_name);
                println!("   Details: {}", details);
                println!("   Options: {:?}", options);
                
                // Simulate Zed's interactive approval
                let decision = adapter.handle_hitl_approval(&session_id, &request_id, &tool_name, &details).await?;
                println!("   Decision: {:?}", decision);
            }
            AgentEvent::Done { output, stats, .. } => {
                println!("\n✅ HITL demo completed!");
                println!("📄 Output: {}", output);
                println!("📊 Stats: {} tools, {} HITL requests", stats.tool_calls, stats.hitl_requests);
                break;
            }
            AgentEvent::Error { message, .. } => {
                eprintln!("\n❌ HITL demo error: {}", message);
                break;
            }
            _ => {}
        }
    }
    
    adapter.client_mut().disconnect().await?;
    Ok(())
}

#[cfg(feature = "zed-acp")]
async fn advanced_zed_features() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🚀 Advanced Zed Features Demo");
    
    let config = ClientConfig::default();
    let adapter = ZedACPAdapter::new(config, "advanced_user".to_string());
    
    // In Phase 2, these features will be implemented:
    
    // 1. File context extraction
    println!("📁 [Phase 2] File context extraction would work like:");
    println!("   - Scan workspace for relevant files");
    println!("   - Extract symbols and dependencies");
    println!("   - Build context-aware prompts");
    
    // 2. Real-time code updates
    println!("⚡ [Phase 2] Real-time code updates would provide:");
    println!("   - Live diff preview in Zed");
    println!("   - Incremental code changes");
    println!("   - Undo/redo integration");
    
    // 3. Error diagnostics integration
    println!("🔍 [Phase 2] Error diagnostics integration:");
    println!("   - LSP error information");
    println!("   - Automatic fix suggestions");
    println!("   - Test failure analysis");
    
    // 4. Symbol-aware prompting
    println!("🎯 [Phase 2] Symbol-aware prompting:");
    println!("   - Function signature context");
    println!("   - Type information");
    println!("   - Import/dependency analysis");
    
    println!("   Current adapter state: {:?}", adapter.client().state());
    
    Ok(())
}

#[cfg(feature = "zed-acp")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();
    
    println!("🎨 AgentLoop Rust Bridge - Zed ACP Integration Example");
    println!("🎯 This demonstrates how Zed editor would integrate with AgentLoop\n");
    
    // Check if AgentLoop server is running
    {
        let config = ClientConfig::default();
        let mut test_client = agentloop_bridge::AgentLoopClient::new(config);
        if test_client.connect().await.is_err() {
            eprintln!("❌ Cannot connect to AgentLoop server!");
            eprintln!("   Please start the server first:");
            eprintln!("   cd ../agentloop && ./agentloop-server &");
            return Ok(());
        }
        test_client.disconnect().await?;
    }
    
    // Run different demo scenarios
    println!("1️⃣  Running basic Zed coding session simulation...");
    simulate_zed_coding_session().await?;
    
    println!("\n2️⃣  Running interactive HITL approval demo...");
    interactive_hitl_demo().await?;
    
    println!("\n3️⃣  Showcasing advanced Zed features (Phase 2)...");
    advanced_zed_features().await?;
    
    println!("\n✨ Zed integration example completed!");
    println!("🔮 Full Zed ACP integration will be available in Phase 2");
    println!("   - Interactive UI elements");
    println!("   - Real-time code updates");
    println!("   - Enhanced file context");
    println!("   - LSP error integration");
    
    Ok(())
}