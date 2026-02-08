use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use auth_manager::{
    api, cluster, config::Config, expiration, storage::Database, AppState,
};
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

    // Initialize cluster state
    let cluster_state = cluster::ClusterState::new(&config, &db)?;
    
    // Create shared state
    let state = Arc::new(AppState {
        db,
        config: config.clone(),
        cluster: RwLock::new(cluster_state),
    });

    // Start background tasks
    let expiration_handle = expiration::start_expiration_cleaner(Arc::clone(&state));
    
    // Start cluster tasks (heartbeat, election) if in cluster mode
    let cluster_handle = if !config.cluster.peers.is_empty() {
        Some(cluster::start_cluster_tasks(Arc::clone(&state)))
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
