//! auth-manager - A tiny, fault-tolerant distributed token management service
//!
//! This crate provides session token and API key management with:
//! - Opaque token generation with User Agent device tracking
//! - Active expiration via background tasks
//! - Leader-follower replication with automatic leader election
//! - Synchronous quorum writes
//! - redb embedded database (ACID, MVCC, crash-safe)
//! - REST API

pub mod api;
pub mod cluster;
pub mod config;
pub mod device;
pub mod expiration;
pub mod storage;
#[cfg(test)]
pub mod testutil;
pub mod tokens;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tokio::sync::RwLock;

use config::Config;
use storage::Database;

/// Shared application state
pub struct AppState {
    pub cluster: RwLock<cluster::ClusterState>,
    pub config: Config,
    pub db: Database,
    /// HTTP client retained for leader-forwarding (proxying write requests)
    pub http_client: reqwest::Client,
    /// Guards against concurrent sync/catchup operations
    pub sync_in_progress: AtomicBool,
    /// TCP transport for inter-node cluster communication
    pub transport: Arc<cluster::ClusterTransport>,
}
