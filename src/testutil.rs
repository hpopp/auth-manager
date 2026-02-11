//! Shared test helpers — available to all `#[cfg(test)]` modules in the crate.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use chrono::Utc;
use tempfile::TempDir;
use tokio::sync::RwLock;

use crate::cluster::ClusterState;
use crate::config::{ClusterConfig, Config, NodeConfig, TokenConfig};
use crate::storage::models::{ApiKey, DeviceInfo, SessionToken};
use crate::storage::Database;
use crate::AppState;

/// Open a fresh database in a temporary directory.
///
/// Returns both the `Database` and the `TempDir` guard — the caller must
/// keep the `TempDir` alive for the duration of the test.
pub fn setup_db() -> (Database, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db = Database::open(temp_dir.path()).unwrap();
    (db, temp_dir)
}

/// A minimal `Config` suitable for unit tests (single-node, no discovery).
pub fn test_config() -> Config {
    Config {
        cluster: ClusterConfig::default(),
        node: NodeConfig {
            bind_address: "127.0.0.1:8080".to_string(),
            data_dir: "/tmp/test".to_string(),
            id: "test-node".to_string(),
        },
        test_mode: false,
        tokens: TokenConfig::default(),
    }
}

/// Build a full `Arc<AppState>` around the given database.
///
/// Uses [`test_config`] and a `reqwest::Client` with proxy disabled
/// (avoids macOS system-configuration panics in sandboxed tests).
pub fn test_state(db: Database) -> Arc<AppState> {
    let config = test_config();
    let cluster_state = ClusterState::new(&config, &db).unwrap();
    let http_client = reqwest::Client::builder().no_proxy().build().unwrap();
    let transport = crate::cluster::ClusterTransport::new(config.cluster.cluster_port);
    Arc::new(AppState {
        cluster: RwLock::new(cluster_state),
        config,
        db,
        http_client,
        sync_in_progress: AtomicBool::new(false),
        transport,
    })
}

/// Create a `SessionToken` with the given id and subject.
pub fn make_session(id: &str, subject: &str) -> SessionToken {
    let now = Utc::now();
    SessionToken {
        created_at: now,
        device_info: DeviceInfo::default(),
        expires_at: now + chrono::Duration::hours(24),
        id: id.to_string(),
        ip_address: None,
        last_used_at: None,
        metadata: None,
        subject_id: subject.to_string(),
        token: format!("tok_{id}"),
    }
}

/// Create an `ApiKey` with the given id and subject.
pub fn make_api_key(id: &str, subject: &str) -> ApiKey {
    let now = Utc::now();
    ApiKey {
        created_at: now,
        description: None,
        expires_at: None,
        id: id.to_string(),
        key_hash: format!("hash_{id}"),
        last_used_at: None,
        name: format!("key-{id}"),
        subject_id: subject.to_string(),
        scopes: vec![],
        updated_at: Some(now),
    }
}
