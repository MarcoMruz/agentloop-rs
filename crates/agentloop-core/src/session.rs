//! Session management for AgentLoop
//! 
//! Manages session lifecycle, state machine, and limits.
//! Equivalent to Go version's internal/session package.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock, Mutex};
use uuid::Uuid;
use chrono::{DateTime, Utc};

use crate::{Result, AgentLoopError, config::SessionConfig};

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

/// Session information
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
    /// Source (cli, slack, etc.)
    pub source: String,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last activity timestamp
    pub last_activity: DateTime<Utc>,
    /// Optional result message
    pub result: Option<String>,
}

/// Messages that can be sent to a session
#[derive(Debug)]
pub enum SessionMessage {
    /// Steer the session with new input
    Steer(String),
    /// Abort the session
    Abort,
    /// Resolve a pending HITL request
    ResolveMitl { request_id: String, decision: HITLDecision },
}

/// HITL decision
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HITLDecision {
    Approve,
    Deny,
    Abort,
}

/// Session manager handles all session lifecycle operations
#[derive(Debug)]
pub struct SessionManager {
    config: SessionConfig,
    sessions: Arc<RwLock<HashMap<SessionId, SessionEntry>>>,
    user_sessions: Arc<RwLock<HashMap<String, Vec<SessionId>>>>,
    session_counter: Arc<Mutex<usize>>,
}

/// Internal session entry with channels
#[derive(Debug)]
struct SessionEntry {
    session: Session,
    message_tx: mpsc::UnboundedSender<SessionMessage>,
    hitl_resolvers: HashMap<String, oneshot::Sender<HITLDecision>>,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            user_sessions: Arc::new(RwLock::new(HashMap::new())),
            session_counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Start a new session
    pub async fn start_session(
        &self,
        user_id: String,
        task: String,
        work_dir: Option<String>,
        source: String,
    ) -> Result<(SessionId, mpsc::UnboundedReceiver<SessionMessage>)> {
        // Check concurrent session limits
        self.enforce_limits(&user_id).await?;

        // Generate session ID
        let session_id = self.generate_session_id().await;
        
        // Create session
        let session = Session {
            id: session_id.clone(),
            user_id: user_id.clone(),
            state: SessionState::Starting,
            task,
            work_dir,
            source,
            created_at: Utc::now(),
            last_activity: Utc::now(),
            result: None,
        };

        // Create message channel
        let (message_tx, message_rx) = mpsc::unbounded_channel();

        // Create session entry
        let entry = SessionEntry {
            session,
            message_tx,
            hitl_resolvers: HashMap::new(),
        };

        // Add to maps
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(session_id.clone(), entry);
        }

        {
            let mut user_sessions = self.user_sessions.write().await;
            user_sessions
                .entry(user_id.clone())
                .or_insert_with(Vec::new)
                .push(session_id.clone());
        }

        tracing::info!("Started session {} for user {}", session_id, user_id);

        Ok((session_id, message_rx))
    }

    /// Update session state
    pub async fn update_state(&self, session_id: &str, new_state: SessionState) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        if let Some(entry) = sessions.get_mut(session_id) {
            entry.session.state = new_state;
            entry.session.last_activity = Utc::now();
            tracing::debug!("Session {} state updated to {:?}", session_id, entry.session.state);
            Ok(())
        } else {
            Err(AgentLoopError::fatal(format!("Session {} not found", session_id)))
        }
    }

    /// Touch session to update last activity
    pub async fn touch_session(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        if let Some(entry) = sessions.get_mut(session_id) {
            entry.session.last_activity = Utc::now();
            Ok(())
        } else {
            Err(AgentLoopError::fatal(format!("Session {} not found", session_id)))
        }
    }

    /// Get session info
    pub async fn get_session(&self, session_id: &str) -> Result<Session> {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .map(|entry| entry.session.clone())
            .ok_or_else(|| AgentLoopError::fatal(format!("Session {} not found", session_id)))
    }

    /// List sessions for a user
    pub async fn list_user_sessions(&self, user_id: &str) -> Result<Vec<Session>> {
        let user_sessions = self.user_sessions.read().await;
        let sessions = self.sessions.read().await;
        
        let mut result = Vec::new();
        if let Some(session_ids) = user_sessions.get(user_id) {
            for session_id in session_ids {
                if let Some(entry) = sessions.get(session_id) {
                    result.push(entry.session.clone());
                }
            }
        }
        
        Ok(result)
    }

    /// Send message to session
    pub async fn send_message(&self, session_id: &str, message: SessionMessage) -> Result<()> {
        let sessions = self.sessions.read().await;
        if let Some(entry) = sessions.get(session_id) {
            entry.message_tx
                .send(message)
                .map_err(|_| AgentLoopError::retryable("Failed to send message to session"))?;
            Ok(())
        } else {
            Err(AgentLoopError::fatal(format!("Session {} not found", session_id)))
        }
    }

    /// Set pending HITL request
    pub async fn set_pending_hitl(&self, session_id: &str, request_id: String) -> Result<oneshot::Receiver<HITLDecision>> {
        let mut sessions = self.sessions.write().await;
        if let Some(entry) = sessions.get_mut(session_id) {
            let (tx, rx) = oneshot::channel();
            entry.hitl_resolvers.insert(request_id, tx);
            self.update_state_internal(&mut entry.session, SessionState::WaitingHitl);
            Ok(rx)
        } else {
            Err(AgentLoopError::fatal(format!("Session {} not found", session_id)))
        }
    }

    /// Resolve HITL request
    pub async fn resolve_hitl(&self, session_id: &str, request_id: &str, decision: HITLDecision) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        if let Some(entry) = sessions.get_mut(session_id) {
            if let Some(resolver) = entry.hitl_resolvers.remove(request_id) {
                let _ = resolver.send(decision);
                if entry.hitl_resolvers.is_empty() {
                    self.update_state_internal(&mut entry.session, SessionState::Running);
                }
                Ok(())
            } else {
                Err(AgentLoopError::fatal(format!("HITL request {} not found for session {}", request_id, session_id)))
            }
        } else {
            Err(AgentLoopError::fatal(format!("Session {} not found", session_id)))
        }
    }

    /// Remove session (cleanup)
    pub async fn remove_session(&self, session_id: &str) -> Result<()> {
        let session = {
            let mut sessions = self.sessions.write().await;
            sessions.remove(session_id)
        };

        if let Some(entry) = session {
            let mut user_sessions = self.user_sessions.write().await;
            if let Some(user_session_list) = user_sessions.get_mut(&entry.session.user_id) {
                user_session_list.retain(|id| id != session_id);
                if user_session_list.is_empty() {
                    user_sessions.remove(&entry.session.user_id);
                }
            }
            tracing::info!("Removed session {}", session_id);
        }

        Ok(())
    }

    /// Get session count
    pub async fn session_count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.len()
    }

    /// Enforce session limits
    async fn enforce_limits(&self, user_id: &str) -> Result<()> {
        // Check total concurrent limit
        {
            let sessions = self.sessions.read().await;
            if sessions.len() >= self.config.max_concurrent {
                if self.config.evict_lru {
                    drop(sessions);
                    self.evict_oldest_lru().await?;
                } else {
                    return Err(AgentLoopError::retryable("Maximum concurrent sessions reached"));
                }
            }
        }

        // Check per-user limit
        {
            let user_sessions = self.user_sessions.read().await;
            let user_session_count = user_sessions
                .get(user_id)
                .map(|list| list.len())
                .unwrap_or(0);

            if user_session_count >= self.config.max_per_user {
                return Err(AgentLoopError::retryable("Maximum sessions per user reached"));
            }
        }

        Ok(())
    }

    /// Evict oldest LRU session
    async fn evict_oldest_lru(&self) -> Result<()> {
        let sessions = self.sessions.read().await;
        
        // Find oldest session by last_activity
        let oldest_id = sessions
            .iter()
            .min_by_key(|(_, entry)| entry.session.last_activity)
            .map(|(id, _)| id.clone());

        drop(sessions);

        if let Some(session_id) = oldest_id {
            // Abort the oldest session
            self.send_message(&session_id, SessionMessage::Abort).await?;
            // Note: actual removal will happen when the session task completes
            tracing::warn!("Evicted oldest session: {}", session_id);
        }

        Ok(())
    }

    /// Generate unique session ID
    async fn generate_session_id(&self) -> SessionId {
        let mut counter = self.session_counter.lock().await;
        *counter += 1;
        let uuid = Uuid::new_v4();
        format!("sess-{}", &uuid.to_string()[..8])
    }

    /// Update state internal (no lock)
    fn update_state_internal(&self, session: &mut Session, new_state: SessionState) {
        session.state = new_state;
        session.last_activity = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SessionConfig;

    fn test_config() -> SessionConfig {
        SessionConfig {
            max_concurrent: 2,
            max_per_user: 1,
            timeout_seconds: 3600,
            token_budget: 100000,
            tool_call_limit: 1000,
            stuck_threshold_seconds: 300,
            evict_lru: false,
        }
    }

    #[tokio::test]
    async fn test_session_lifecycle() {
        let manager = SessionManager::new(test_config());
        
        // Start session
        let (session_id, _rx) = manager
            .start_session("user1".to_string(), "test task".to_string(), None, "test".to_string())
            .await
            .unwrap();

        // Check session exists
        let session = manager.get_session(&session_id).await.unwrap();
        assert_eq!(session.state, SessionState::Starting);
        assert_eq!(session.user_id, "user1");

        // Update state
        manager.update_state(&session_id, SessionState::Running).await.unwrap();
        let session = manager.get_session(&session_id).await.unwrap();
        assert_eq!(session.state, SessionState::Running);

        // Remove session
        manager.remove_session(&session_id).await.unwrap();
        assert!(manager.get_session(&session_id).await.is_err());
    }

    #[tokio::test]
    async fn test_session_limits() {
        let manager = SessionManager::new(test_config());
        
        // Start one session (within limit)
        let (_id1, _rx1) = manager
            .start_session("user1".to_string(), "task1".to_string(), None, "test".to_string())
            .await
            .unwrap();

        // Try to start another session for same user (should fail)
        let result = manager
            .start_session("user1".to_string(), "task2".to_string(), None, "test".to_string())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().is_retryable());
    }

    #[tokio::test]
    async fn test_hitl_workflow() {
        let manager = SessionManager::new(test_config());
        
        // Start session
        let (session_id, _rx) = manager
            .start_session("user1".to_string(), "test task".to_string(), None, "test".to_string())
            .await
            .unwrap();

        // Set pending HITL
        let request_id = "test-request".to_string();
        let hitl_rx = manager.set_pending_hitl(&session_id, request_id.clone()).await.unwrap();
        
        // Session should be in waiting state
        let session = manager.get_session(&session_id).await.unwrap();
        assert_eq!(session.state, SessionState::WaitingHitl);

        // Resolve HITL
        manager.resolve_hitl(&session_id, &request_id, HITLDecision::Approve).await.unwrap();
        
        // Session should be running again
        let session = manager.get_session(&session_id).await.unwrap();
        assert_eq!(session.state, SessionState::Running);

        // HITL resolver should have received decision
        let decision = hitl_rx.await.unwrap();
        assert!(matches!(decision, HITLDecision::Approve));
    }
}