//! Advanced event handling example for AgentLoop Rust bridge
//!
//! This example demonstrates:
//! - Advanced event processing patterns
//! - Multiple concurrent tasks
//! - Event filtering and categorization
//! - Statistics collection
//! - Error recovery

use agentloop_bridge::{AgentEvent, AgentLoopClient, ClientConfig, HITLDecision, TaskStats};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::time::timeout;

#[derive(Debug, Default)]
struct EventStatistics {
    total_events: u32,
    text_chunks: u32,
    tool_uses: u32,
    tool_results: u32,
    hitl_requests: u32,
    errors: u32,
    completions: u32,
    total_text_length: usize,
    start_time: Option<Instant>,
    end_time: Option<Instant>,
}

impl EventStatistics {
    fn record_event(&mut self, event: &AgentEvent) {
        if self.start_time.is_none() {
            self.start_time = Some(Instant::now());
        }
        
        self.total_events += 1;
        
        match event {
            AgentEvent::Text { content, .. } => {
                self.text_chunks += 1;
                self.total_text_length += content.len();
            }
            AgentEvent::ToolUse { .. } => self.tool_uses += 1,
            AgentEvent::ToolResult { .. } => self.tool_results += 1,
            AgentEvent::HITLRequest { .. } => self.hitl_requests += 1,
            AgentEvent::Error { .. } => self.errors += 1,
            AgentEvent::Done { .. } => {
                self.completions += 1;
                self.end_time = Some(Instant::now());
            }
            _ => {}
        }
    }
    
    fn duration(&self) -> Option<Duration> {
        match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => Some(end - start),
            _ => None,
        }
    }
    
    fn events_per_second(&self) -> f64 {
        if let Some(duration) = self.duration() {
            self.total_events as f64 / duration.as_secs_f64()
        } else {
            0.0
        }
    }
}

struct TaskManager {
    active_tasks: HashMap<String, TaskInfo>,
    completed_tasks: Vec<CompletedTask>,
}

#[derive(Debug)]
struct TaskInfo {
    description: String,
    start_time: Instant,
    tool_calls: u32,
    hitl_requests: u32,
}

#[derive(Debug)]
struct CompletedTask {
    session_id: String,
    description: String,
    duration: Duration,
    stats: TaskStats,
    final_output: String,
}

impl TaskManager {
    fn new() -> Self {
        Self {
            active_tasks: HashMap::new(),
            completed_tasks: Vec::new(),
        }
    }
    
    fn start_task(&mut self, session_id: String, description: String) {
        let info = TaskInfo {
            description,
            start_time: Instant::now(),
            tool_calls: 0,
            hitl_requests: 0,
        };
        self.active_tasks.insert(session_id, info);
    }
    
    fn record_tool_use(&mut self, session_id: &str) {
        if let Some(info) = self.active_tasks.get_mut(session_id) {
            info.tool_calls += 1;
        }
    }
    
    fn record_hitl(&mut self, session_id: &str) {
        if let Some(info) = self.active_tasks.get_mut(session_id) {
            info.hitl_requests += 1;
        }
    }
    
    fn complete_task(&mut self, session_id: String, output: String, stats: TaskStats) {
        if let Some(info) = self.active_tasks.remove(&session_id) {
            let completed = CompletedTask {
                session_id,
                description: info.description,
                duration: info.start_time.elapsed(),
                stats,
                final_output: output,
            };
            self.completed_tasks.push(completed);
        }
    }
    
    fn active_count(&self) -> usize {
        self.active_tasks.len()
    }
    
    fn completed_count(&self) -> usize {
        self.completed_tasks.len()
    }
}

async fn run_multiple_tasks(client: &mut AgentLoopClient) -> Result<(), Box<dyn std::error::Error>> {
    let tasks = vec![
        ("List all Rust files in the current directory", "ls *.rs || find . -name '*.rs'"),
        ("Show current Git status", "git status --porcelain"),
        ("Count lines of code", "find . -name '*.rs' -exec wc -l {} +"),
    ];
    
    println!("🚀 Starting {} concurrent tasks...", tasks.len());
    
    let mut session_ids = Vec::new();
    
    for (i, (description, _)) in tasks.into_iter().enumerate() {
        let session_id = client.start_task(
            "batch_user",
            description.to_string(),
            Some(std::env::current_dir()?.to_string_lossy().to_string()),
            "batch_example"
        ).await?;

        println!("📝 Task {}: {} ({})", i + 1, description, session_id);
        session_ids.push(session_id);
        
        // Small delay to avoid overwhelming the server
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging with more detailed output
    tracing_subscriber::fmt()
        .with_env_filter("debug")
        .init();
    
    println!("📊 AgentLoop Rust Bridge - Advanced Event Handling Example");
    
    // Create client
    let config = ClientConfig::default();
    let mut client = AgentLoopClient::new(config);
    let mut event_rx = client.take_event_receiver().unwrap();
    
    // Statistics tracking
    let mut stats = EventStatistics::default();
    let mut task_manager = TaskManager::new();
    
    // Connect with timeout
    println!("📡 Connecting to AgentLoop server...");
    timeout(Duration::from_secs(10), client.connect()).await??;
    println!("✅ Connected successfully!");
    
    // Start multiple tasks
    run_multiple_tasks(&mut client).await?;
    
    // Event processing loop with timeout
    println!("👂 Processing events (timeout: 60s)...\n");
    
    let event_timeout = Duration::from_secs(60);
    let start_time = Instant::now();
    
    loop {
        // Check for timeout
        if start_time.elapsed() > event_timeout {
            println!("⏰ Event processing timed out after {}s", event_timeout.as_secs());
            break;
        }
        
        // Wait for event with short timeout for status updates
        match timeout(Duration::from_secs(1), event_rx.recv()).await {
            Ok(Some(event)) => {
                // Record statistics
                stats.record_event(&event);
                
                // Process event
                match event {
                    AgentEvent::Text { session_id, content } => {
                        // Show abbreviated text output
                        let preview = if content.len() > 100 {
                            format!("{}...", &content[..100])
                        } else {
                            content.clone()
                        };
                        println!("📝 [{}] {}", &session_id[..8], preview.replace('\n', "\\n"));
                    }
                    AgentEvent::ToolUse { session_id, tool_name, input } => {
                        task_manager.record_tool_use(&session_id);
                        println!("🔧 [{}] {} {:?}", &session_id[..8], tool_name, input);
                    }
                    AgentEvent::ToolResult { session_id, tool_name, output, success } => {
                        let status = if success { "✅" } else { "❌" };
                        let preview = output.lines().next().unwrap_or("").to_string();
                        println!("{} [{}] {} result: {}", status, &session_id[..8], tool_name, preview);
                    }
                    AgentEvent::HITLAutoApproved { session_id, tool_name, risk_level, command, .. } => {
                        println!("✅ [{}] Auto-approved [risk: {}] {}: {}", &session_id[..8], risk_level, tool_name, command);
                    }
                    AgentEvent::HITLRequest { session_id, request_id, tool_name, details, risk_level, .. } => {
                        task_manager.record_hitl(&session_id);
                        let risk = risk_level.as_deref().unwrap_or("unknown");
                        println!("🤔 [{}] HITL [risk: {}]: {} - {}", &session_id[..8], risk, tool_name, details);
                        
                        // Smart approval logic
                        let decision = if tool_name == "bash" {
                            if details.contains("rm") || details.contains("delete") || details.contains(">") {
                                println!("   ❌ Denying potentially dangerous command");
                                HITLDecision::Deny
                            } else {
                                println!("   ✅ Approving safe command");
                                HITLDecision::Approve
                            }
                        } else {
                            println!("   ✅ Approving {} operation", tool_name);
                            HITLDecision::Approve
                        };
                        
                        if let Err(e) = client.respond_hitl(&session_id, &request_id, decision).await {
                            eprintln!("❌ HITL response failed: {}", e);
                        }
                    }
                    AgentEvent::Done { session_id, output, stats: task_stats } => {
                        task_manager.complete_task(session_id.clone(), output.clone(), task_stats);
                        println!("🎉 [{}] Completed: {}", &session_id[..8], 
                                output.lines().next().unwrap_or(""));
                        
                        // Check if all tasks are complete
                        if task_manager.active_count() == 0 && task_manager.completed_count() > 0 {
                            println!("\n✨ All tasks completed!");
                            break;
                        }
                    }
                    AgentEvent::Error { session_id, message } => {
                        println!("❌ [{}] Error: {}", &session_id[..8], message);
                    }
                    AgentEvent::StateChanged(state) => {
                        println!("🔄 Connection state: {:?}", state);
                    }
                    AgentEvent::SessionSaved { session_id } => {
                        println!("💾 [{}] Session saved", &session_id[..8]);
                    }
                }
                
                // Show periodic statistics
                if stats.total_events % 10 == 0 {
                    println!("📊 Stats: {} events, {} active tasks, {} completed", 
                            stats.total_events, task_manager.active_count(), task_manager.completed_count());
                }
            }
            Ok(None) => {
                println!("📡 Event stream ended");
                break;
            }
            Err(_) => {
                // Timeout - show status update
                println!("📊 Status: {} events, {} active tasks, {} completed, {}s elapsed", 
                        stats.total_events, 
                        task_manager.active_count(), 
                        task_manager.completed_count(),
                        start_time.elapsed().as_secs());
                continue;
            }
        }
    }
    
    // Final statistics report
    println!("\n📈 Final Statistics:");
    println!("   Total events: {}", stats.total_events);
    println!("   Text chunks: {}", stats.text_chunks);
    println!("   Tool uses: {}", stats.tool_uses);
    println!("   Tool results: {}", stats.tool_results);
    println!("   HITL requests: {}", stats.hitl_requests);
    println!("   Errors: {}", stats.errors);
    println!("   Completions: {}", stats.completions);
    println!("   Total text length: {} chars", stats.total_text_length);
    
    if let Some(duration) = stats.duration() {
        println!("   Duration: {:.2}s", duration.as_secs_f64());
        println!("   Events per second: {:.1}", stats.events_per_second());
    }
    
    println!("\n📋 Task Summary:");
    for (i, task) in task_manager.completed_tasks.iter().enumerate() {
        println!("   {}. {} ({:.2}s, {} tools, {} HITL)", 
                i + 1, 
                task.description, 
                task.duration.as_secs_f64(),
                task.stats.tool_calls,
                task.stats.hitl_requests);
    }
    
    // Cleanup
    println!("\n👋 Disconnecting...");
    client.disconnect().await?;
    println!("✅ Example completed successfully!");
    
    Ok(())
}