use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use auth_manager::{
    api, config::Config, expiration, state_machine::AuthStateMachine, storage::Database, AppState,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    let env_filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    let log_format = std::env::var("LOG_FORMAT").unwrap_or_default();
    match log_format.to_lowercase().as_str() {
        "gcp" => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_stackdriver::layer())
                .init();
        }
        "json" => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_target(true)
                        .with_span_list(false),
                )
                .init();
        }
        _ => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer())
                .init();
        }
    }

    info!(version = env!("CARGO_PKG_VERSION"), "auth-manager starting");

    // Load configuration
    let config = Config::load()?;
    info!("Loaded configuration for node: {}", config.node.id);

    // Initialize database
    let db = Database::open(&config.node.data_dir)?;
    info!("Database opened at: {}", config.node.data_dir);

    // Build muster configuration from auth-manager config.
    // Peer addresses are rewritten from HTTP port to cluster TCP port since
    // muster operates entirely in cluster-TCP land.
    let cluster_port = config.cluster.cluster_port;
    let cluster_peers: Vec<String> = config
        .cluster
        .peers
        .iter()
        .map(|peer| {
            if let Some((host, _)) = peer.rsplit_once(':') {
                format!("{host}:{cluster_port}")
            } else {
                format!("{peer}:{cluster_port}")
            }
        })
        .collect();

    let muster_config = muster::Config {
        node_id: config.node.id.clone(),
        cluster_port,
        heartbeat_interval_ms: config.cluster.heartbeat_interval_ms,
        election_timeout_ms: config.cluster.election_timeout_ms,
        discovery: muster::DiscoveryConfig {
            dns_name: config.cluster.discovery.dns_name.clone(),
            peers: cluster_peers,
            poll_interval_secs: config.cluster.discovery.poll_interval_seconds,
        },
    };

    // Create muster storage (shares the redb instance with auth-manager)
    let muster_storage = muster::RedbStorage::new(db.inner())?;

    // Create the state machine
    let state_machine = AuthStateMachine::new(db.clone());

    // Create the cluster node
    let node = muster::MusterNode::new(muster_config, muster_storage, state_machine)?;

    // Start cluster background tasks (heartbeat, election, discovery, TCP server)
    let cluster_handles = node.start();

    // Create shared state
    let state = Arc::new(AppState {
        config: config.clone(),
        db,
        node: Arc::clone(&node),
    });

    // Start background tasks
    let expiration_handle = expiration::start_expiration_cleaner(Arc::clone(&state));

    // Build and start the HTTP server — bind before initial discovery
    // so health probes can respond immediately
    let app = api::create_router(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind(&config.node.bind_address).await?;
    info!("Listening on: {}", config.node.bind_address);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Cleanup: abort background tasks
    info!("Shutting down background tasks");
    expiration_handle.abort();
    for handle in cluster_handles {
        handle.abort();
    }

    // Persist cluster state to disk
    if let Err(e) = node.persist_state().await {
        tracing::error!(error = %e, "Failed to persist cluster state during shutdown");
    }

    info!("Shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received, draining connections");
}
