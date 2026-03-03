//! AgentLoop Core Library
//! 
//! This crate contains shared types and utilities for AgentLoop client bridge, including:
//! - Session types and management utilities
//! - Configuration system
//! - Error taxonomy

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod config;
pub mod errors;
pub mod session;

pub use config::Config;
pub use errors::{Result, AgentLoopError};