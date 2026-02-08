use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to parse config: {0}")]
    ParseError(#[from] toml::de::Error),
    #[error("Failed to read config file: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("Invalid configuration: {0}")]
    ValidationError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub cluster: ClusterConfig,
    pub node: NodeConfig,
    #[serde(default)]
    pub tokens: TokenConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    #[serde(default = "default_bind_address")]
    pub bind_address: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    #[serde(default)]
    pub discovery: DiscoveryConfig,
    #[serde(default = "default_election_timeout_ms")]
    pub election_timeout_ms: u64,
    #[serde(default = "default_heartbeat_interval_ms")]
    pub heartbeat_interval_ms: u64,
    #[serde(default = "default_log_retention_days")]
    pub log_retention_days: u64,
    #[serde(default = "default_log_retention_entries")]
    pub log_retention_entries: u64,
    #[serde(default)]
    pub peers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// DNS name to resolve for peer discovery (required for dns strategy)
    #[serde(default)]
    pub dns_name: Option<String>,
    /// How often to poll for peer changes (seconds)
    #[serde(default = "default_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    /// Discovery strategy: "dns" or "static"
    #[serde(default)]
    pub strategy: DiscoveryStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DiscoveryStrategy {
    Dns,
    #[default]
    Static,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenConfig {
    #[serde(default = "default_cleanup_interval_seconds")]
    pub cleanup_interval_seconds: u64,
    #[serde(default = "default_session_ttl_seconds")]
    pub session_ttl_seconds: u64,
}

fn default_bind_address() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_data_dir() -> String {
    "./data".to_string()
}

fn default_heartbeat_interval_ms() -> u64 {
    150 // Send heartbeat every 150ms
}

fn default_election_timeout_ms() -> u64 {
    1500 // Wait 1.5s before triggering election (more forgiving for Docker networking)
}

fn default_log_retention_entries() -> u64 {
    100_000
}

fn default_log_retention_days() -> u64 {
    7
}

fn default_poll_interval_seconds() -> u64 {
    5
}

fn default_session_ttl_seconds() -> u64 {
    86400 // 24 hours
}

fn default_cleanup_interval_seconds() -> u64 {
    60 // 1 minute
}

impl Default for TokenConfig {
    fn default() -> Self {
        Self {
            cleanup_interval_seconds: default_cleanup_interval_seconds(),
            session_ttl_seconds: default_session_ttl_seconds(),
        }
    }
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            dns_name: None,
            poll_interval_seconds: default_poll_interval_seconds(),
            strategy: DiscoveryStrategy::Static,
        }
    }
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            discovery: DiscoveryConfig::default(),
            election_timeout_ms: default_election_timeout_ms(),
            heartbeat_interval_ms: default_heartbeat_interval_ms(),
            log_retention_days: default_log_retention_days(),
            log_retention_entries: default_log_retention_entries(),
            peers: Vec::new(),
        }
    }
}

impl Config {
    /// Load configuration from file or environment
    pub fn load() -> Result<Self, ConfigError> {
        // Try to load from config.toml in current directory
        let config_path =
            std::env::var("AUTH_MANAGER_CONFIG").unwrap_or_else(|_| "config.toml".to_string());

        if Path::new(&config_path).exists() {
            let contents = fs::read_to_string(&config_path)?;
            let config: Config = toml::from_str(&contents)?;
            config.validate()?;
            Ok(config)
        } else {
            // Use defaults with node ID from environment or generate one
            let node_id = std::env::var("AUTH_MANAGER_NODE_ID")
                .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());

            let bind_address = std::env::var("AUTH_MANAGER_BIND_ADDRESS")
                .unwrap_or_else(|_| default_bind_address());

            let data_dir =
                std::env::var("AUTH_MANAGER_DATA_DIR").unwrap_or_else(|_| default_data_dir());

            let peers: Vec<String> = std::env::var("AUTH_MANAGER_PEERS")
                .map(|p| {
                    p.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        // Filter out self - peer address starts with node_id (e.g., "node-1:8080" starts with "node-1")
                        .filter(|s| !s.starts_with(&format!("{}:", &node_id)) && s != &node_id)
                        .collect()
                })
                .unwrap_or_default();

            // Discovery configuration from environment
            let dns_name = std::env::var("AUTH_MANAGER_DISCOVERY_DNS_NAME").ok();
            let discovery_strategy = if dns_name.is_some() {
                DiscoveryStrategy::Dns
            } else {
                std::env::var("AUTH_MANAGER_DISCOVERY_STRATEGY")
                    .ok()
                    .and_then(|s| match s.to_lowercase().as_str() {
                        "dns" => Some(DiscoveryStrategy::Dns),
                        _ => Some(DiscoveryStrategy::Static),
                    })
                    .unwrap_or(DiscoveryStrategy::Static)
            };
            let poll_interval = std::env::var("AUTH_MANAGER_DISCOVERY_POLL_INTERVAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(default_poll_interval_seconds());

            Ok(Config {
                node: NodeConfig {
                    id: node_id,
                    bind_address,
                    data_dir,
                },
                cluster: ClusterConfig {
                    peers,
                    discovery: DiscoveryConfig {
                        strategy: discovery_strategy,
                        dns_name,
                        poll_interval_seconds: poll_interval,
                    },
                    ..Default::default()
                },
                tokens: TokenConfig::default(),
            })
        }
    }

    /// Validate the configuration
    fn validate(&self) -> Result<(), ConfigError> {
        if self.node.id.is_empty() {
            return Err(ConfigError::ValidationError(
                "node.id cannot be empty".to_string(),
            ));
        }

        let cluster_size = self.cluster.peers.len() + 1;
        if cluster_size > 1 && cluster_size % 2 == 0 {
            tracing::warn!(
                "Cluster size {} is even. This may lead to split-brain scenarios. \
                 Consider using an odd number of nodes.",
                cluster_size
            );
        }

        Ok(())
    }

    /// Calculate the quorum size for the cluster
    pub fn quorum_size(&self) -> usize {
        let cluster_size = self.cluster.peers.len() + 1;
        cluster_size / 2 + 1
    }

    /// Check if running in single-node mode
    ///
    /// Returns false if either static peers or DNS discovery is configured.
    pub fn is_single_node(&self) -> bool {
        self.cluster.peers.is_empty() && self.cluster.discovery.dns_name.is_none()
    }
}
