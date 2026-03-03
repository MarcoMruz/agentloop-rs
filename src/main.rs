//! AgentLoop workspace main entry point
//! 
//! This is primarily for development and testing purposes.
//! The actual binaries are in crates/agentloop-server and crates/agentloop-cli.

use anyhow::Result;

fn main() -> Result<()> {
    println!("AgentLoop Rust Implementation");
    println!("=============================");
    println!();
    println!("Available binaries:");
    println!("  agentloop-server  - Long-running server process");
    println!("  agentloop         - CLI client");
    println!();
    println!("To build:");
    println!("  cargo build --release");
    println!();
    println!("To run server:");
    println!("  cargo run --bin agentloop-server");
    println!();
    println!("To run CLI:");
    println!("  cargo run --bin agentloop -- \"your task here\"");
    println!();
    println!("For development, use individual crate directories:");
    println!("  cd crates/agentloop-server && cargo run");
    println!("  cd crates/agentloop-cli && cargo run -- --help");
    
    Ok(())
}