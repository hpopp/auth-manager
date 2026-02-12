//! Follower catch-up mechanism for syncing new/lagging nodes

use std::sync::atomic::Ordering;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info};

use super::rpc::{ClusterMessage, Snapshot, SyncRequest, SyncResponse};
use crate::storage::models::WriteOp;
use crate::storage::Database;
use crate::AppState;

#[derive(Debug, Error)]
pub enum CatchupError {
    #[error("Database error: {0}")]
    Database(#[from] crate::storage::DatabaseError),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Sync already in progress")]
    AlreadySyncing,
}

/// Request sync from the leader.
///
/// Acquires the `sync_in_progress` guard so only one sync runs at a time.
pub async fn request_sync(state: Arc<AppState>, leader_addr: &str) -> Result<(), CatchupError> {
    // Acquire sync guard — bail if another sync is already running
    if state
        .sync_in_progress
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        debug!("Sync already in progress, skipping.");
        return Err(CatchupError::AlreadySyncing);
    }

    let result = do_sync(Arc::clone(&state), leader_addr).await;

    // Always release the guard
    state.sync_in_progress.store(false, Ordering::SeqCst);

    result
}

/// Inner sync logic (separated so the guard release happens in the caller).
async fn do_sync(state: Arc<AppState>, leader_addr: &str) -> Result<(), CatchupError> {
    let my_sequence = {
        let cluster = state.cluster.read().await;
        cluster.last_applied_sequence
    };

    info!(my_sequence, leader = %leader_addr, "Requesting sync from leader.");

    // Request sync from leader over TCP
    let response = send_sync_request(&state.transport, leader_addr, my_sequence).await?;

    // Apply snapshot if provided
    if let Some(ref snapshot) = response.snapshot {
        apply_snapshot(&state.db, snapshot)?;
        info!(sequence = snapshot.sequence, "Applied snapshot.");
    }

    // Apply log entries
    let entry_count = response.log_entries.len();
    for entry in &response.log_entries {
        apply_log_entry(&state.db, entry)?;
    }

    // Update our sequence
    {
        let mut cluster = state.cluster.write().await;
        cluster.last_applied_sequence = response.leader_sequence;
    }

    info!(
        sequence = response.leader_sequence,
        entries = entry_count,
        "Sync complete."
    );
    Ok(())
}

/// Send a sync request to the leader over TCP.
async fn send_sync_request(
    transport: &super::ClusterTransport,
    leader_addr: &str,
    from_sequence: u64,
) -> Result<SyncResponse, CatchupError> {
    let request = SyncRequest { from_sequence };

    let response = transport
        .send(leader_addr, ClusterMessage::SyncRequest(request))
        .await
        .map_err(|e| CatchupError::Network(format!("Failed to contact leader: {e}")))?;

    match response {
        ClusterMessage::SyncResponse(resp) => Ok(*resp),
        other => Err(CatchupError::Network(format!(
            "Unexpected sync response: {other:?}"
        ))),
    }
}

/// Apply a snapshot to the local database
fn apply_snapshot(db: &Database, snapshot: &Snapshot) -> Result<(), CatchupError> {
    // Apply sessions
    for session in &snapshot.sessions {
        db.put_session(session)?;
    }

    // Apply API keys
    for api_key in &snapshot.api_keys {
        db.put_api_key(api_key)?;
    }

    debug!(
        sessions = snapshot.sessions.len(),
        api_keys = snapshot.api_keys.len(),
        "Snapshot applied."
    );

    Ok(())
}

/// Apply a single log entry to the local database
fn apply_log_entry(
    db: &Database,
    entry: &crate::storage::models::ReplicatedWrite,
) -> Result<(), CatchupError> {
    match &entry.operation {
        WriteOp::CreateSession(session) => {
            db.put_session(session)?;
        }
        WriteOp::RevokeSession { token_id } => {
            db.delete_session(token_id)?;
        }
        WriteOp::CreateApiKey(api_key) => {
            db.put_api_key(api_key)?;
        }
        WriteOp::RevokeApiKey { key_id } => {
            db.delete_api_key(key_id)?;
        }
        WriteOp::UpdateApiKey {
            key_hash,
            description,
            name,
            scopes,
        } => {
            db.update_api_key(
                key_hash,
                name.as_deref(),
                description.as_ref().map(|d| d.as_deref()),
                scopes.as_deref(),
            )?;
        }
    }

    // Record in replication log
    db.append_replication_log(entry)?;

    Ok(())
}

/// Handle a sync request from a follower (leader only).
pub async fn handle_sync_request(
    state: Arc<AppState>,
    from_sequence: u64,
) -> Result<SyncResponse, CatchupError> {
    let leader_sequence = state.db.get_latest_sequence()?;

    // If follower is brand new or far behind, send a full snapshot
    let lag = leader_sequence.saturating_sub(from_sequence);
    let needs_snapshot = from_sequence == 0 || lag > 10_000;

    let snapshot = if needs_snapshot {
        Some(create_snapshot(&state.db)?)
    } else {
        None
    };

    // Get log entries from after the snapshot (or from the follower's sequence)
    let start_sequence = snapshot
        .as_ref()
        .map(|s| s.sequence + 1)
        .unwrap_or(from_sequence + 1);

    let log_entries = state.db.get_replication_log_from(start_sequence)?;

    info!(
        from_sequence,
        leader_sequence,
        snapshot = snapshot.is_some(),
        log_entries = log_entries.len(),
        "Serving sync request."
    );

    Ok(SyncResponse {
        snapshot,
        log_entries,
        leader_sequence,
    })
}

/// Create a snapshot of the current state
fn create_snapshot(db: &Database) -> Result<Snapshot, CatchupError> {
    let sequence = db.get_latest_sequence()?;
    let sessions = db.get_all_sessions()?;
    let api_keys = db.get_all_api_keys()?;

    debug!(
        sequence,
        sessions = sessions.len(),
        api_keys = api_keys.len(),
        "Created snapshot."
    );

    Ok(Snapshot {
        sequence,
        sessions,
        api_keys,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::models::ReplicatedWrite;
    use crate::testutil::{make_api_key, make_session, setup_db, test_state};
    use chrono::Utc;

    // ========================================================================
    // create_snapshot + apply_snapshot
    // ========================================================================

    #[test]
    fn test_create_snapshot_empty_db() {
        let (db, _temp) = setup_db();

        let snapshot = create_snapshot(&db).unwrap();
        assert_eq!(snapshot.sequence, 0);
        assert!(snapshot.sessions.is_empty());
        assert!(snapshot.api_keys.is_empty());
    }

    #[test]
    fn test_create_snapshot_with_data() {
        let (db, _temp) = setup_db();

        db.put_session(&make_session("s1", "user1")).unwrap();
        db.put_session(&make_session("s2", "user1")).unwrap();
        db.put_api_key(&make_api_key("k1", "user1")).unwrap();

        let snapshot = create_snapshot(&db).unwrap();
        assert_eq!(snapshot.sessions.len(), 2);
        assert_eq!(snapshot.api_keys.len(), 1);
    }

    #[test]
    fn test_apply_snapshot_to_empty_db() {
        let (source_db, _temp1) = setup_db();
        let (target_db, _temp2) = setup_db();

        // Populate source
        source_db.put_session(&make_session("s1", "user1")).unwrap();
        source_db.put_api_key(&make_api_key("k1", "user1")).unwrap();

        let snapshot = create_snapshot(&source_db).unwrap();

        // Apply to empty target
        apply_snapshot(&target_db, &snapshot).unwrap();

        assert_eq!(target_db.get_all_sessions().unwrap().len(), 1);
        assert_eq!(target_db.get_all_api_keys().unwrap().len(), 1);
    }

    #[test]
    fn test_snapshot_roundtrip_preserves_data() {
        let (source_db, _temp1) = setup_db();
        let (target_db, _temp2) = setup_db();

        let session = make_session("s1", "user1");
        let api_key = make_api_key("k1", "user1");

        source_db.put_session(&session).unwrap();
        source_db.put_api_key(&api_key).unwrap();

        let snapshot = create_snapshot(&source_db).unwrap();
        apply_snapshot(&target_db, &snapshot).unwrap();

        let sessions = target_db.get_all_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "s1");
        assert_eq!(sessions[0].subject_id, "user1");

        let keys = target_db.get_all_api_keys().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].id, "k1");
        assert_eq!(keys[0].name, "key-k1");
    }

    // ========================================================================
    // apply_log_entry
    // ========================================================================

    #[test]
    fn test_apply_log_entry_create_session() {
        let (db, _temp) = setup_db();

        let session = make_session("s1", "user1");
        let entry = ReplicatedWrite {
            sequence: 1,
            operation: WriteOp::CreateSession(session.clone()),
            timestamp: Utc::now(),
        };

        apply_log_entry(&db, &entry).unwrap();

        let sessions = db.get_all_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "s1");

        // Verify it was recorded in the replication log
        let log = db.get_replication_log_from(1).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].sequence, 1);
    }

    #[test]
    fn test_apply_log_entry_revoke_session() {
        let (db, _temp) = setup_db();

        let session = make_session("s1", "user1");
        db.put_session(&session).unwrap();

        let entry = ReplicatedWrite {
            sequence: 1,
            operation: WriteOp::RevokeSession {
                token_id: session.token.clone(),
            },
            timestamp: Utc::now(),
        };

        apply_log_entry(&db, &entry).unwrap();

        let sessions = db.get_all_sessions().unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_apply_log_entry_create_api_key() {
        let (db, _temp) = setup_db();

        let api_key = make_api_key("k1", "user1");
        let entry = ReplicatedWrite {
            sequence: 1,
            operation: WriteOp::CreateApiKey(api_key.clone()),
            timestamp: Utc::now(),
        };

        apply_log_entry(&db, &entry).unwrap();

        let keys = db.get_all_api_keys().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].id, "k1");
    }

    #[test]
    fn test_apply_log_entry_revoke_api_key() {
        let (db, _temp) = setup_db();

        let api_key = make_api_key("k1", "user1");
        db.put_api_key(&api_key).unwrap();

        let entry = ReplicatedWrite {
            sequence: 1,
            operation: WriteOp::RevokeApiKey {
                key_id: api_key.key_hash.clone(),
            },
            timestamp: Utc::now(),
        };

        apply_log_entry(&db, &entry).unwrap();

        let keys = db.get_all_api_keys().unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn test_apply_multiple_log_entries() {
        let (db, _temp) = setup_db();

        let entries = vec![
            ReplicatedWrite {
                sequence: 1,
                operation: WriteOp::CreateSession(make_session("s1", "user1")),
                timestamp: Utc::now(),
            },
            ReplicatedWrite {
                sequence: 2,
                operation: WriteOp::CreateApiKey(make_api_key("k1", "user1")),
                timestamp: Utc::now(),
            },
            ReplicatedWrite {
                sequence: 3,
                operation: WriteOp::CreateSession(make_session("s2", "user2")),
                timestamp: Utc::now(),
            },
        ];

        for entry in &entries {
            apply_log_entry(&db, entry).unwrap();
        }

        assert_eq!(db.get_all_sessions().unwrap().len(), 2);
        assert_eq!(db.get_all_api_keys().unwrap().len(), 1);
        assert_eq!(db.get_latest_sequence().unwrap(), 3);
    }

    // ========================================================================
    // handle_sync_request
    // ========================================================================

    #[tokio::test]
    async fn test_handle_sync_request_brand_new_follower() {
        let (db, _temp) = setup_db();

        // Populate leader with data and log entries
        let session = make_session("s1", "user1");
        db.put_session(&session).unwrap();
        db.put_api_key(&make_api_key("k1", "user1")).unwrap();

        let write = ReplicatedWrite {
            sequence: 1,
            operation: WriteOp::CreateSession(session),
            timestamp: Utc::now(),
        };
        db.append_replication_log(&write).unwrap();

        let state = test_state(db);

        // from_sequence=0 means brand new follower, should get a snapshot
        let response = handle_sync_request(state, 0).await.unwrap();

        assert!(response.snapshot.is_some());
        let snapshot = response.snapshot.unwrap();
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.api_keys.len(), 1);
        assert_eq!(response.leader_sequence, 1);
    }

    #[tokio::test]
    async fn test_handle_sync_request_slightly_behind() {
        let (db, _temp) = setup_db();

        // Create 5 log entries
        for i in 1..=5 {
            let write = ReplicatedWrite {
                sequence: i,
                operation: WriteOp::CreateSession(make_session(&format!("s{i}"), "user1")),
                timestamp: Utc::now(),
            };
            db.append_replication_log(&write).unwrap();
            if let WriteOp::CreateSession(ref s) = write.operation {
                db.put_session(s).unwrap();
            }
        }

        let state = test_state(db);

        // Follower has sequence 3 — should get incremental (entries 4 and 5), no snapshot
        let response = handle_sync_request(state, 3).await.unwrap();

        assert!(response.snapshot.is_none());
        assert_eq!(response.log_entries.len(), 2);
        assert_eq!(response.log_entries[0].sequence, 4);
        assert_eq!(response.log_entries[1].sequence, 5);
        assert_eq!(response.leader_sequence, 5);
    }

    #[tokio::test]
    async fn test_handle_sync_request_already_up_to_date() {
        let (db, _temp) = setup_db();

        let write = ReplicatedWrite {
            sequence: 1,
            operation: WriteOp::CreateSession(make_session("s1", "user1")),
            timestamp: Utc::now(),
        };
        db.append_replication_log(&write).unwrap();

        let state = test_state(db);

        // Follower is at sequence 1, same as leader
        let response = handle_sync_request(state, 1).await.unwrap();

        assert!(response.snapshot.is_none());
        assert!(response.log_entries.is_empty());
        assert_eq!(response.leader_sequence, 1);
    }

    // ========================================================================
    // Sync guard
    // ========================================================================

    #[tokio::test]
    async fn test_sync_guard_prevents_concurrent_sync() {
        let (db, _temp) = setup_db();
        let state = test_state(db);

        // Manually set the guard
        state.sync_in_progress.store(true, Ordering::SeqCst);

        // Attempt sync should fail with AlreadySyncing
        let result = request_sync(Arc::clone(&state), "127.0.0.1:9999").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CatchupError::AlreadySyncing));

        // Guard should still be set (we set it manually, request_sync didn't touch it)
        assert!(state.sync_in_progress.load(Ordering::SeqCst));
    }
}
