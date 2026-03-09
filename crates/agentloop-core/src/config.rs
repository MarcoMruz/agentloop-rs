//! Client configuration for AgentLoop bridge

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Client configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
}

/// Server connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Unix socket path (default: ~/.local/share/agentloop/agentloop.sock)
    pub socket_path: PathBuf,
}

impl Config {
    /// Load configuration from default path with environment variable overrides
    pub fn load() -> crate::Result<Self> {
        Self::load_from_path(None)
    }

    /// Load configuration from specific path
    pub fn load_from_path(config_path: Option<PathBuf>) -> crate::Result<Self> {
        let mut settings = config::Config::builder();

        settings = settings.add_source(config::Config::try_from(&Self::defaults())?);

        let path = config_path.unwrap_or_else(Self::default_config_path);
        if path.exists() {
            settings = settings.add_source(config::File::from(path));
        }

        settings = settings.add_source(
            config::Environment::with_prefix("AGENTLOOP")
                .separator("_")
                .try_parsing(true),
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
        }
    }

    fn default_config_path() -> PathBuf {
        Self::home_dir().join(".config/agentloop/agentloop.yaml")
    }

    fn home_dir() -> PathBuf {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
    }

    fn expand_paths(mut config: Self) -> Self {
        config.server.socket_path = Self::expand_home_dir(config.server.socket_path);
        config
    }

    fn expand_home_dir(path: PathBuf) -> PathBuf {
        if let Some(s) = path.to_str() {
            if s.starts_with("~/") {
                return Self::home_dir().join(&s[2..]);
            }
        }
        path
    }
}
