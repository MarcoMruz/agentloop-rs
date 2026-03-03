//! Configuration system for AgentLoop
//! 
//! Equivalent to the Go version's config package with Viper.
//! Uses the `config` crate for YAML + environment variable support.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Main configuration struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub pi: PiConfig,
    pub vault: VaultConfig,
    pub memory: MemoryConfig,
    pub sessions: SessionConfig,
    pub hitl: HITLConfig,
    pub security: SecurityConfig,
    pub skills: SkillsConfig,
    pub logging: LoggingConfig,
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Unix socket path (default: ~/.local/share/agentloop/agentloop.sock)
    pub socket_path: PathBuf,
}

/// Pi coding agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiConfig {
    /// Path to pi binary (default: "pi")
    pub binary_path: String,
    /// LLM provider (default: "anthropic")
    pub provider: String,
    /// Model name (default: "claude-sonnet-4-20250514")
    pub model: String,
    /// Extensions directory path
    pub extensions_dir: Option<PathBuf>,
    /// Extra arguments to pass to pi
    pub extra_args: Vec<String>,
}

/// Vault storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultConfig {
    /// Vault directory path (default: ~/.local/share/agentloop/vault/)
    pub path: PathBuf,
}

/// Memory management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Maximum entries in user profile
    pub max_profile_entries: usize,
    /// Conversation retention days
    pub conversation_retention_days: u32,
    /// Enable compaction
    pub enable_compaction: bool,
    /// Compaction strategy
    pub compaction_strategy: CompactionStrategy,
    /// Prompt cache TTL in seconds
    pub cache_ttl_seconds: u64,
}

/// Session management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Maximum concurrent sessions
    pub max_concurrent: usize,
    /// Maximum sessions per user
    pub max_per_user: usize,
    /// Session timeout in seconds
    pub timeout_seconds: u64,
    /// Token budget per session
    pub token_budget: u32,
    /// Tool call limit per session
    pub tool_call_limit: u32,
    /// Stuck detection threshold
    pub stuck_threshold_seconds: u64,
    /// Enable LRU eviction
    pub evict_lru: bool,
}

/// HITL (Human-in-the-Loop) configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HITLConfig {
    /// Tools that always require approval
    pub always_pause_tools: Vec<String>,
    /// HITL timeout in seconds
    pub timeout_seconds: u64,
    /// Action on timeout
    pub timeout_action: HITLTimeoutAction,
}

/// Security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Allowed paths for file operations
    pub allowed_paths: Vec<PathBuf>,
    /// Blocked environment variable prefixes
    pub blocked_env_prefixes: Vec<String>,
    /// Blocked CIDR ranges
    pub blocked_cidrs: Vec<String>,
    /// Docker rules
    pub docker: DockerConfig,
    /// Injection protection
    pub injection: InjectionConfig,
}

/// Docker security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerConfig {
    /// Allowed docker subcommands
    pub allowed_subcommands: Vec<String>,
    /// Blocked volume mount paths
    pub blocked_volume_paths: Vec<PathBuf>,
}

/// Prompt injection protection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionConfig {
    /// Enable injection protection
    pub enabled: bool,
    /// Whitelisted source paths
    pub whitelist_sources: Vec<PathBuf>,
    /// Blocked keywords
    pub blocked_keywords: Vec<String>,
    /// Patterns requiring approval
    pub require_approval: Vec<String>,
    /// Maximum content length
    pub max_content_length: usize,
    /// Approval tier
    pub approval_tier: ApprovalTier,
    /// Sanitize memory content
    pub sanitize_memory: bool,
}

/// Skills configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    /// Skill directory paths
    pub directories: Vec<PathBuf>,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level
    pub level: LogLevel,
    /// Log file path (optional)
    pub file_path: Option<PathBuf>,
}

/// Compaction strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionStrategy {
    Rolling,
    Facts,
    Topics,
}

/// HITL timeout action
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HITLTimeoutAction {
    Approve,
    Deny,
    Abort,
}

/// Approval tier for injection protection
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalTier {
    Owner,
    Admin,
    AutoDeny,
}

/// Log level
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl Config {
    /// Load configuration from default path with environment variable overrides
    pub fn load() -> crate::Result<Self> {
        Self::load_from_path(None)
    }

    /// Load configuration from specific path
    pub fn load_from_path(config_path: Option<PathBuf>) -> crate::Result<Self> {
        let mut settings = config::Config::builder();

        // Add default configuration
        settings = settings.add_source(config::Config::try_from(&Self::defaults())?);

        // Add configuration file if it exists
        if let Some(path) = config_path {
            if path.exists() {
                settings = settings.add_source(config::File::from(path));
            }
        } else {
            // Try default config path
            let default_path = Self::default_config_path();
            if default_path.exists() {
                settings = settings.add_source(config::File::from(default_path));
            }
        }

        // Add environment variable overrides with AGENTLOOP_ prefix
        settings = settings.add_source(
            config::Environment::with_prefix("AGENTLOOP")
                .separator("_")
                .try_parsing(true)
        );

        let config = settings.build()?.try_deserialize()?;
        
        Ok(Self::expand_paths(config))
    }

    /// Get default configuration
    pub fn defaults() -> Self {
        Self {
            server: ServerConfig {
                socket_path: Self::home_dir().join(".local/share/agentloop/agentloop.sock"),
            },
            pi: PiConfig {
                binary_path: "pi".to_string(),
                provider: "anthropic".to_string(),
                model: "claude-sonnet-4-20250514".to_string(),
                extensions_dir: None,
                extra_args: vec![],
            },
            vault: VaultConfig {
                path: Self::home_dir().join(".local/share/agentloop/vault/"),
            },
            memory: MemoryConfig {
                max_profile_entries: 100,
                conversation_retention_days: 30,
                enable_compaction: true,
                compaction_strategy: CompactionStrategy::Rolling,
                cache_ttl_seconds: 3600,
            },
            sessions: SessionConfig {
                max_concurrent: 10,
                max_per_user: 5,
                timeout_seconds: 3600,
                token_budget: 100000,
                tool_call_limit: 1000,
                stuck_threshold_seconds: 300,
                evict_lru: true,
            },
            hitl: HITLConfig {
                always_pause_tools: vec!["docker".to_string(), "bash".to_string()],
                timeout_seconds: 300,
                timeout_action: HITLTimeoutAction::Deny,
            },
            security: SecurityConfig {
                allowed_paths: vec![],
                blocked_env_prefixes: vec![
                    "AWS_".to_string(),
                    "GOOGLE_".to_string(),
                    "GITHUB_".to_string(),
                ],
                blocked_cidrs: vec!["127.0.0.0/8".to_string(), "10.0.0.0/8".to_string()],
                docker: DockerConfig {
                    allowed_subcommands: vec![
                        "ps".to_string(),
                        "logs".to_string(),
                        "images".to_string(),
                        "build".to_string(),
                        "compose".to_string(),
                        "inspect".to_string(),
                        "stats".to_string(),
                    ],
                    blocked_volume_paths: vec![
                        "/etc".into(),
                        "/var".into(),
                        "/root".into(),
                        "/proc".into(),
                        "/sys".into(),
                        "/dev".into(),
                    ],
                },
                injection: InjectionConfig {
                    enabled: true,
                    whitelist_sources: vec![],
                    blocked_keywords: vec![
                        "ignore previous instructions".to_string(),
                        "system prompt".to_string(),
                        "assistant mode".to_string(),
                    ],
                    require_approval: vec!["eval".to_string(), "exec".to_string()],
                    max_content_length: 50000,
                    approval_tier: ApprovalTier::Owner,
                    sanitize_memory: true,
                },
            },
            skills: SkillsConfig {
                directories: vec![Self::home_dir().join(".local/share/agentloop/vault/skills/")],
            },
            logging: LoggingConfig {
                level: LogLevel::Info,
                file_path: Some(Self::home_dir().join(".local/share/agentloop/agentloop.log")),
            },
        }
    }

    /// Get default config file path
    fn default_config_path() -> PathBuf {
        Self::home_dir().join(".config/agentloop/agentloop.yaml")
    }

    /// Get home directory
    fn home_dir() -> PathBuf {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
    }

    /// Expand ~ in paths
    fn expand_paths(mut config: Self) -> Self {
        config.server.socket_path = Self::expand_home_dir(config.server.socket_path);
        config.vault.path = Self::expand_home_dir(config.vault.path);
        
        if let Some(ref mut ext_dir) = config.pi.extensions_dir {
            *ext_dir = Self::expand_home_dir(ext_dir.clone());
        }

        config.security.allowed_paths = config
            .security
            .allowed_paths
            .into_iter()
            .map(Self::expand_home_dir)
            .collect();

        config.security.injection.whitelist_sources = config
            .security
            .injection
            .whitelist_sources
            .into_iter()
            .map(Self::expand_home_dir)
            .collect();

        config.skills.directories = config
            .skills
            .directories
            .into_iter()
            .map(Self::expand_home_dir)
            .collect();

        if let Some(ref mut log_path) = config.logging.file_path {
            *log_path = Self::expand_home_dir(log_path.clone());
        }

        config
    }

    /// Expand ~ to home directory in path
    fn expand_home_dir(path: PathBuf) -> PathBuf {
        if let Some(path_str) = path.to_str() {
            if path_str.starts_with("~/") {
                return Self::home_dir().join(&path_str[2..]);
            }
        }
        path
    }
}