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
pub mod tokens;

use tokio::sync::RwLock;

use config::Config;
use storage::Database;

/// Shared application state
pub struct AppState {
    pub db: Database,
    pub config: Config,
    pub cluster: RwLock<cluster::ClusterState>,
}
