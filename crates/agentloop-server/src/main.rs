//! AgentLoop Server
//! 
//! Long-running server that manages sessions, memory, and agent execution via UNIX socket.

use agentloop_core::{Config, server::Server, session::SessionManager};
use anyhow::Result;
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agentloop_server=info,agentloop_core=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting AgentLoop server...");

    // Load configuration
    let config = match Config::load() {
        Ok(config) => {
            info!("Configuration loaded successfully");
            config
        }
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            return Err(e.into());
        }
    };

    // Create session manager
    let session_manager = SessionManager::new(config.sessions.clone());
    info!("Session manager initialized");

    // Create and run server
    let server = match Server::new(&config.server.socket_path, session_manager).await {
        Ok(server) => {
            info!("Server created successfully on {:?}", config.server.socket_path);
            server
        }
        Err(e) => {
            error!("Failed to create server: {}", e);
            return Err(e.into());
        }
    };

    // Handle shutdown gracefully
    tokio::select! {
        result = server.run() => {
            match result {
                Ok(_) => info!("Server shut down normally"),
                Err(e) => {
                    error!("Server error: {}", e);
                    return Err(e.into());
                }
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal, stopping server...");
        }
    }

    Ok(())
}