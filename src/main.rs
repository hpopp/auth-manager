use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use auth_manager::{
    api, cluster, config::Config, expiration, storage::Database, AppState,
};
use cluster::discovery::{Discovery, DnsPoll, StaticList};
use auth_manager::config::DiscoveryStrategy;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            "auth_manager=debug,tower_http=debug".into()
        }))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let config = Config::load()?;
    info!("Loaded configuration for node: {}", config.node.id);

    // Initialize database
    let db = Database::open(&config.node.data_dir)?;
    info!("Database opened at: {}", config.node.data_dir);

    // Build discovery strategy
    let discovery = build_discovery(&config);

    // Initialize cluster state
    let cluster_state = cluster::ClusterState::new(&config, &db)?;
    
    // Create shared state
    let state = Arc::new(AppState {
        db,
        config: config.clone(),
        cluster: RwLock::new(cluster_state),
    });

    // Run initial peer discovery before starting cluster tasks
    if let Some(ref disc) = discovery {
        match disc.discover_peers().await {
            Ok(peers) => {
                let mut cluster = state.cluster.write().await;
                cluster.update_discovered_peers(peers);
                info!("Initial discovery found {} peer(s)", cluster.peer_states.len());
            }
            Err(e) => {
                tracing::warn!(error = %e, "Initial peer discovery failed (will retry in background)");
            }
        }
    }

    // Start background tasks
    let expiration_handle = expiration::start_expiration_cleaner(Arc::clone(&state));
    
    // Start cluster tasks (heartbeat, election, discovery) if in cluster mode
    let cluster_handle = if !config.is_single_node() {
        Some(cluster::start_cluster_tasks(Arc::clone(&state), discovery))
    } else {
        info!("Running in single-node mode (no peers configured)");
        None
    };

    // Build and start the HTTP server
    let app = api::create_router(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind(&config.node.bind_address).await?;
    info!("Listening on: {}", config.node.bind_address);

    axum::serve(listener, app).await?;

    // Cleanup
    expiration_handle.abort();
    if let Some(handle) = cluster_handle {
        handle.abort();
    }

    Ok(())
}

/// Build the appropriate discovery strategy from configuration
fn build_discovery(config: &Config) -> Option<Discovery> {
    if config.is_single_node() {
        return None;
    }

    match config.cluster.discovery.strategy {
        DiscoveryStrategy::Dns => {
            let dns_name = config.cluster.discovery.dns_name.clone()
                .expect("dns_name is required when discovery strategy is 'dns'");
            let port = config.node.bind_address
                .rsplit(':')
                .next()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8080u16);
            Some(Discovery::Dns(DnsPoll::new(dns_name, port)))
        }
        DiscoveryStrategy::Static => {
            if config.cluster.peers.is_empty() {
                None
            } else {
                Some(Discovery::Static(StaticList::new(config.cluster.peers.clone())))
            }
        }
    }
}
