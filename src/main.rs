//! AgentLoop-rs workspace entry point
//!
//! This is a thin Rust client bridge for the AgentLoop Go server.
//! The actual binaries are in crates/agentloop-cli.

use anyhow::Result;

fn main() -> Result<()> {
    println!("AgentLoop Rust Bridge");
    println!("=====================");
    println!();
    println!("This is a client bridge for the AgentLoop Go server.");
    println!("Start the Go server first, then use the CLI to connect.");
    println!();
    println!("To build:");
    println!("  cargo build --release");
    println!();
    println!("To run CLI:");
    println!("  cargo run --bin agentloop -- \"your task here\"");
    println!();
    println!("For development:");
    println!("  cd crates/agentloop-cli && cargo run -- --help");

    Ok(())
}