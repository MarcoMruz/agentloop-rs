//! Session types for AgentLoop client bridge
//! 
//! Contains shared session types used by the bridge client.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Session ID type
pub type SessionId = String;

/// Session state
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Starting,
    Running,
    WaitingHitl,
    Done,
    Aborted,
    Error,
}

/// Session information (read-only for client)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session ID
    pub id: SessionId,
    /// User ID who owns this session
    pub user_id: String,
    /// Current state
    pub state: SessionState,
    /// Task description
    pub task: String,
    /// Working directory
    pub work_dir: Option<String>,
    /// Source (cli, slack, zed, etc.)
    pub source: String,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last activity timestamp
    pub last_activity: DateTime<Utc>,
    /// Optional result message
    pub result: Option<String>,
}

/// Task execution statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStats {
    /// Session ID
    pub session_id: SessionId,
    /// Total execution time in milliseconds
    pub duration_ms: u64,
    /// Number of tool calls made
    pub tool_calls: u32,
    /// Number of HITL requests
    pub hitl_requests: u32,
    /// Number of tokens consumed (if available)
    pub tokens_consumed: Option<u32>,
    /// Final status
    pub status: SessionState,
}

/// HITL decision options
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HITLDecision {
    Approve,
    Deny,
    Abort,
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionState::Starting => write!(f, "starting"),
            SessionState::Running => write!(f, "running"),
            SessionState::WaitingHitl => write!(f, "waiting_hitl"),
            SessionState::Done => write!(f, "done"),
            SessionState::Aborted => write!(f, "aborted"),
            SessionState::Error => write!(f, "error"),
        }
    }
}

impl std::fmt::Display for HITLDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HITLDecision::Approve => write!(f, "approve"),
            HITLDecision::Deny => write!(f, "deny"),
            HITLDecision::Abort => write!(f, "abort"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_state_serialization() {
        let state = SessionState::Running;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"running\"");
        
        let deserialized: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, SessionState::Running);
    }

    #[test]
    fn test_hitl_decision_serialization() {
        let decision = HITLDecision::Approve;
        let json = serde_json::to_string(&decision).unwrap();
        assert_eq!(json, "\"approve\"");
        
        let deserialized: HITLDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, HITLDecision::Approve);
    }

    #[test]
    fn test_session_creation() {
        let session = Session {
            id: "sess-12345".to_string(),
            user_id: "marco".to_string(),
            state: SessionState::Starting,
            task: "test task".to_string(),
            work_dir: Some("/home/marco/project".to_string()),
            source: "test".to_string(),
            created_at: Utc::now(),
            last_activity: Utc::now(),
            result: None,
        };
        
        assert_eq!(session.id, "sess-12345");
        assert_eq!(session.user_id, "marco");
        assert_eq!(session.state, SessionState::Starting);
    }
}