//! AgentLoop Core Library
//! 
//! This crate contains all the core business logic for AgentLoop, including:
//! - UNIX socket server with JSON-RPC 2.0 support
//! - Session management and lifecycle
//! - Configuration system
//! - Error taxonomy
//! - Logging setup

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod config;
pub mod errors;
pub mod server;
pub mod session;

pub use config::Config;
pub use errors::{Result, AgentLoopError};