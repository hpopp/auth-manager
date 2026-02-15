//! auth-manager - A tiny, fault-tolerant distributed token management service
//!
//! This crate provides session token and API key management with:
//! - Opaque token generation with User Agent device tracking
//! - Active expiration via background tasks
//! - Leader-follower replication with automatic leader election (via muster)
//! - Synchronous quorum writes
//! - redb embedded database (ACID, MVCC, crash-safe)
//! - REST API

pub mod api;
pub mod config;
pub mod device;
pub mod expiration;
pub mod state_machine;
pub mod storage;
#[cfg(test)]
pub mod testutil;
pub mod tokens;

use std::sync::Arc;

use config::Config;
use state_machine::AuthStateMachine;
use storage::Database;

/// Shared application state
pub struct AppState {
    pub config: Config,
    pub db: Database,
    pub node: Arc<muster::RedbNode<AuthStateMachine>>,
}
