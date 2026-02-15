use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Invalid configuration: {0}")]
    ValidationError(String),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub cluster: ClusterConfig,
    pub node: NodeConfig,
    /// Enables dangerous operations like purge. Must never be true in production.
    pub test_mode: bool,
    pub tokens: TokenConfig,
}

#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub bind_address: String,
    pub data_dir: String,
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct ClusterConfig {
    /// TCP port for inter-node cluster communication
    pub cluster_port: u16,
    pub discovery: DiscoveryConfig,
    pub election_timeout_ms: u64,
    pub heartbeat_interval_ms: u64,
    pub log_retention_days: u64,
    pub log_retention_entries: u64,
    pub peers: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    /// DNS name to resolve for peer discovery (required for dns strategy)
    pub dns_name: Option<String>,
    /// How often to poll for peer changes (seconds)
    pub poll_interval_seconds: u64,
    /// Discovery strategy: "dns" or "static"
    pub strategy: DiscoveryStrategy,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum DiscoveryStrategy {
    Dns,
    #[default]
    Static,
}

#[derive(Debug, Clone)]
pub struct TokenConfig {
    pub cleanup_interval_seconds: u64,
    pub session_ttl_seconds: u64,
}

impl Default for TokenConfig {
    fn default() -> Self {
        Self {
            cleanup_interval_seconds: 60,
            session_ttl_seconds: 86400, // 24 hours
        }
    }
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            dns_name: None,
            poll_interval_seconds: 5,
            strategy: DiscoveryStrategy::Static,
        }
    }
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            cluster_port: 9993,
            discovery: DiscoveryConfig::default(),
            election_timeout_ms: 3000,
            heartbeat_interval_ms: 300,
            log_retention_days: 7,
            log_retention_entries: 100_000,
            peers: Vec::new(),
        }
    }
}

impl Config {
    /// Load configuration from environment variables.
    pub fn load() -> Result<Self, ConfigError> {
        let node_id = std::env::var("NODE_ID").unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());

        let bind_address =
            std::env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

        let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());

        let peers: Vec<String> = std::env::var("PEERS")
            .map(|p| {
                p.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .filter(|s| !s.starts_with(&format!("{node_id}:")) && s != &node_id)
                    .collect()
            })
            .unwrap_or_default();

        let dns_name = std::env::var("DISCOVERY_DNS_NAME").ok();
        let discovery_strategy = if dns_name.is_some() {
            DiscoveryStrategy::Dns
        } else {
            std::env::var("DISCOVERY_STRATEGY")
                .ok()
                .map(|s| match s.to_lowercase().as_str() {
                    "dns" => DiscoveryStrategy::Dns,
                    _ => DiscoveryStrategy::Static,
                })
                .unwrap_or(DiscoveryStrategy::Static)
        };
        let poll_interval = std::env::var("DISCOVERY_POLL_INTERVAL")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);

        let cluster_port = std::env::var("CLUSTER_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(9993);

        let test_mode = std::env::var("TEST_MODE")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let config = Config {
            node: NodeConfig {
                id: node_id,
                bind_address,
                data_dir,
            },
            cluster: ClusterConfig {
                cluster_port,
                peers,
                discovery: DiscoveryConfig {
                    strategy: discovery_strategy,
                    dns_name,
                    poll_interval_seconds: poll_interval,
                },
                ..Default::default()
            },
            test_mode,
            tokens: TokenConfig::default(),
        };

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.node.id.is_empty() {
            return Err(ConfigError::ValidationError(
                "NODE_ID cannot be empty".to_string(),
            ));
        }

        let cluster_size = self.cluster.peers.len() + 1;
        if cluster_size > 1 && cluster_size.is_multiple_of(2) {
            tracing::warn!(
                "Cluster size {} is even. This may lead to split-brain scenarios. \
                 Consider using an odd number of nodes.",
                cluster_size
            );
        }

        Ok(())
    }

    /// Check if running in single-node mode.
    ///
    /// Returns false if either static peers or DNS discovery is configured.
    pub fn is_single_node(&self) -> bool {
        self.cluster.peers.is_empty() && self.cluster.discovery.dns_name.is_none()
    }
}
