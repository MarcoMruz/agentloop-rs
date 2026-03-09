//! AgentLoop Core Library
//!
//! Shared types and configuration for the AgentLoop client bridge.

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod config;
pub mod errors;

pub use config::Config;
pub use errors::{AgentLoopError, Result};
