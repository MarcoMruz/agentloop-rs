//! Error taxonomy for AgentLoop client bridge

use thiserror::Error;

/// Result type alias for AgentLoop operations
pub type Result<T> = std::result::Result<T, AgentLoopError>;

/// Main error type for AgentLoop operations
#[derive(Error, Debug)]
pub enum AgentLoopError {
    /// Retryable errors - transient failures that can be retried
    #[error("Retryable error: {message}")]
    Retryable {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Fatal errors - stop immediately, no retry
    #[error("Fatal error: {message}")]
    Fatal {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// User abort - user cancelled operation
    #[error("User aborted: {message}")]
    UserAbort { message: String },

    /// Tool failure - tool execution error
    #[error("Tool failed: {message}")]
    ToolFailure {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// IO errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization errors
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Configuration errors
    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),
}

impl AgentLoopError {
    /// Create a retryable error
    pub fn retryable(message: impl Into<String>) -> Self {
        Self::Retryable {
            message: message.into(),
            source: None,
        }
    }

    /// Create a retryable error with source
    pub fn retryable_with_source(
        message: impl Into<String>, 
        source: impl std::error::Error + Send + Sync + 'static
    ) -> Self {
        Self::Retryable {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Create a fatal error
    pub fn fatal(message: impl Into<String>) -> Self {
        Self::Fatal {
            message: message.into(),
            source: None,
        }
    }

    /// Create a fatal error with source
    pub fn fatal_with_source(
        message: impl Into<String>, 
        source: impl std::error::Error + Send + Sync + 'static
    ) -> Self {
        Self::Fatal {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Create a user abort error
    pub fn user_abort(message: impl Into<String>) -> Self {
        Self::UserAbort {
            message: message.into(),
        }
    }

    /// Create a tool failure error
    pub fn tool_failure(message: impl Into<String>) -> Self {
        Self::ToolFailure {
            message: message.into(),
            source: None,
        }
    }

    /// Create a tool failure error with source
    pub fn tool_failure_with_source(
        message: impl Into<String>, 
        source: impl std::error::Error + Send + Sync + 'static
    ) -> Self {
        Self::ToolFailure {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(self, AgentLoopError::Retryable { .. })
    }

    /// Check if error is user abort
    pub fn is_user_abort(&self) -> bool {
        matches!(self, AgentLoopError::UserAbort { .. })
    }

    /// Check if error is fatal
    pub fn is_fatal(&self) -> bool {
        matches!(self, AgentLoopError::Fatal { .. })
    }

    /// Check if error is tool failure
    pub fn is_tool_failure(&self) -> bool {
        matches!(self, AgentLoopError::ToolFailure { .. })
    }
}