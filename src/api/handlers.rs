use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::cluster::{replicate_write, ReplicationError, Role};
use crate::device::parse_user_agent;
use crate::storage::models::{ApiKey as ApiKeyModel, ReplicatedWrite, SessionToken, WriteOp};
use crate::tokens::{
    api_key,
    generator::{generate_api_key, generate_token, hash_key},
    session,
};
use crate::AppState;

use super::response::{ApiError, JSend, JSendFail};

// ============================================================================
// Leader forwarding helper
// ============================================================================

/// Forward a JSON request to the leader node
async fn forward_to_leader<
    T: serde::Serialize,
    R: serde::Serialize + serde::de::DeserializeOwned,
>(
    state: &Arc<AppState>,
    method: &str,
    path: &str,
    body: Option<&T>,
) -> Result<R, ApiError> {
    let cluster = state.cluster.read().await;

    let leader_addr = cluster.get_leader_address().ok_or_else(|| {
        ApiError::unavailable("Leader unknown. Cluster may be electing a new leader.")
    })?;
    drop(cluster);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ApiError::internal(format!("Failed to create HTTP client: {}", e)))?;

    let url = format!("http://{}{}", leader_addr, path);

    let request_builder = match method {
        "POST" => client.post(&url),
        "DELETE" => client.delete(&url),
        "GET" => client.get(&url),
        _ => client.get(&url),
    };

    let request_builder = if let Some(b) = body {
        request_builder.json(b)
    } else {
        request_builder
    };

    let response = request_builder.send().await.map_err(|e| {
        ApiError::bad_gateway(format!("Failed to forward request to leader: {}", e))
    })?;

    let status = response.status();

    if status.is_success() {
        // Leader returns JSend-wrapped responses; unwrap the envelope
        let envelope = response
            .json::<JSend<R>>()
            .await
            .map_err(|e| ApiError::internal(format!("Failed to parse leader response: {}", e)))?;
        Ok(envelope.data)
    } else {
        // Try to parse a JSend fail/error response from leader
        let error_msg = response
            .json::<JSendFail>()
            .await
            .map(|f| f.data.message)
            .unwrap_or_else(|_| format!("Leader returned status: {}", status));

        Err(ApiError::from_status(
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            error_msg,
        ))
    }
}

/// Forward a DELETE request to the leader (expects no body in response)
async fn forward_delete_to_leader(state: &Arc<AppState>, path: &str) -> Result<(), ApiError> {
    let cluster = state.cluster.read().await;

    let leader_addr = cluster.get_leader_address().ok_or_else(|| {
        ApiError::unavailable("Leader unknown. Cluster may be electing a new leader.")
    })?;
    drop(cluster);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ApiError::internal(format!("Failed to create HTTP client: {}", e)))?;

    let url = format!("http://{}{}", leader_addr, path);

    let response = client.delete(&url).send().await.map_err(|e| {
        ApiError::bad_gateway(format!("Failed to forward request to leader: {}", e))
    })?;

    let status = response.status();

    if status.is_success() {
        Ok(())
    } else {
        let error_msg = response
            .text()
            .await
            .unwrap_or_else(|_| format!("Leader returned status: {}", status));

        Err(ApiError::from_status(
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            error_msg,
        ))
    }
}

/// Map a ReplicationError to an ApiError
fn replication_error(e: ReplicationError) -> ApiError {
    match e {
        ReplicationError::NotLeader => {
            ApiError::unavailable("Not the leader. Forward request to leader.")
        }
        ReplicationError::NoQuorum => {
            ApiError::unavailable("Failed to reach quorum for replication")
        }
        _ => ApiError::internal(e.to_string()),
    }
}

// ============================================================================
// Request/Response types
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateSessionRequest {
    pub resource_id: String,
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
    #[serde(default)]
    pub user_agent: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSessionResponse {
    pub expires_at: String,
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub created_at: String,
    pub device_info: DeviceInfoResponse,
    pub expires_at: String,
    pub resource_id: String,
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct DeviceInfoResponse {
    pub browser: Option<String>,
    pub browser_version: Option<String>,
    pub kind: String,
    pub os: Option<String>,
    pub os_version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ParseUserAgentRequest {
    pub user_agent: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateApiKeyRequest {
    #[serde(default)]
    pub expires_in_days: Option<u64>,
    pub name: String,
    pub resource_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateApiKeyResponse {
    pub expires_at: Option<String>,
    pub id: String,
    pub key: String,
    pub name: String,
    pub resource_id: String,
}

#[derive(Debug, Serialize)]
pub struct ApiKeyResponse {
    pub created_at: String,
    pub expires_at: Option<String>,
    pub id: String,
    pub name: String,
    pub resource_id: String,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub node_id: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct ClusterStatusResponse {
    pub cluster_size: usize,
    pub node_id: String,
    pub peers: Vec<PeerStatus>,
    pub quorum: usize,
    pub role: String,
    pub sequence: u64,
    pub term: u64,
}

#[derive(Debug, Serialize)]
pub struct PeerStatus {
    pub id: String,
    pub sequence: u64,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct PurgeResponse {
    pub api_keys_deleted: u64,
    pub replication_entries_deleted: u64,
    pub sessions_deleted: u64,
}

// ============================================================================
// Internal replication types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ReplicateRequest {
    pub leader_id: String,
    pub operation: WriteOp,
    pub sequence: u64,
    pub term: u64,
}

#[derive(Debug, Serialize)]
pub struct ReplicateResponse {
    pub sequence: u64,
    pub success: bool,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct HeartbeatRequest {
    /// Leader's advertise address (for follower forwarding)
    #[serde(default)]
    pub leader_address: Option<String>,
    pub leader_id: String,
    pub sequence: u64,
    pub term: u64,
}

#[derive(Debug, Serialize)]
pub struct HeartbeatResponse {
    pub sequence: u64,
    pub success: bool,
    pub term: u64,
}

#[derive(Debug, Deserialize)]
pub struct VoteRequest {
    pub candidate_id: String,
    pub last_sequence: u64,
    pub term: u64,
}

#[derive(Debug, Serialize)]
pub struct VoteResponse {
    pub term: u64,
    pub vote_granted: bool,
}

// ============================================================================
// Session handlers
// ============================================================================

pub async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<JSend<CreateSessionResponse>>, ApiError> {
    // Check if we're the leader (in cluster mode) - forward to leader if not
    if !state.config.is_single_node() {
        let cluster = state.cluster.read().await;
        if cluster.role != Role::Leader {
            drop(cluster);
            let response: CreateSessionResponse =
                forward_to_leader(&state, "POST", "/sessions", Some(&req)).await?;
            return Ok(JSend::success(response));
        }
    }

    let device_info = req
        .user_agent
        .as_deref()
        .map(parse_user_agent)
        .unwrap_or_default();

    let ttl = req
        .ttl_seconds
        .unwrap_or(state.config.tokens.session_ttl_seconds);

    // Create the session token
    let now = Utc::now();
    let session = SessionToken {
        id: generate_token(),
        resource_id: req.resource_id.clone(),
        created_at: now,
        expires_at: now + chrono::Duration::seconds(ttl as i64),
        device_info,
    };

    // Replicate to cluster (includes local write)
    let operation = WriteOp::CreateSession(session.clone());
    replicate_write(Arc::clone(&state), operation)
        .await
        .map_err(replication_error)?;

    // Apply locally after successful replication
    state
        .db
        .put_session(&session)
        .map_err(|e| ApiError::internal(format!("Failed to store session: {}", e)))?;

    tracing::debug!(token_id = %session.id, resource_id = %req.resource_id, "Created session token");

    Ok(JSend::success(CreateSessionResponse {
        token: session.id,
        expires_at: session.expires_at.to_rfc3339(),
    }))
}

pub async fn validate_session(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<Json<JSend<SessionResponse>>, ApiError> {
    match session::validate(&state.db, &token) {
        Ok(Some(session)) => Ok(JSend::success(session_to_response(&session))),
        Ok(None) => Err(ApiError::not_found("Session not found or expired")),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

pub async fn revoke_session(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<Json<JSend<()>>, ApiError> {
    // Check if we're the leader (in cluster mode) - forward to leader if not
    if !state.config.is_single_node() {
        let cluster = state.cluster.read().await;
        if cluster.role != Role::Leader {
            drop(cluster);
            let path = format!("/sessions/{}", token);
            forward_delete_to_leader(&state, &path).await?;
            return Ok(JSend::success(()));
        }
    }

    // Check if session exists first
    match state.db.get_session(&token) {
        Ok(Some(_)) => {}
        Ok(None) => return Err(ApiError::not_found("Session not found")),
        Err(e) => return Err(ApiError::internal(e.to_string())),
    }

    // Replicate revocation to cluster
    let operation = WriteOp::RevokeSession {
        token_id: token.clone(),
    };
    replicate_write(Arc::clone(&state), operation)
        .await
        .map_err(replication_error)?;

    // Apply locally after successful replication
    state
        .db
        .delete_session(&token)
        .map_err(|e| ApiError::internal(format!("Failed to delete session: {}", e)))?;

    tracing::debug!(token_id = %token, "Revoked session token");
    Ok(JSend::success(()))
}

pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
    Path(resource_id): Path<String>,
) -> Result<Json<JSend<Vec<SessionResponse>>>, ApiError> {
    match session::list_by_resource(&state.db, &resource_id) {
        Ok(sessions) => Ok(JSend::success(
            sessions.iter().map(session_to_response).collect(),
        )),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

// ============================================================================
// API key handlers
// ============================================================================

pub async fn create_api_key(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateApiKeyRequest>,
) -> Result<Json<JSend<CreateApiKeyResponse>>, ApiError> {
    // Check if we're the leader (in cluster mode) - forward to leader if not
    if !state.config.is_single_node() {
        let cluster = state.cluster.read().await;
        if cluster.role != Role::Leader {
            drop(cluster);
            let response: CreateApiKeyResponse =
                forward_to_leader(&state, "POST", "/api-keys", Some(&req)).await?;
            return Ok(JSend::success(response));
        }
    }

    // Generate the API key
    let key = generate_api_key();
    let key_hash = hash_key(&key);
    let now = Utc::now();

    let api_key_record = ApiKeyModel {
        created_at: now,
        expires_at: req
            .expires_in_days
            .map(|days| now + chrono::Duration::days(days as i64)),
        id: uuid::Uuid::new_v4().to_string(),
        key_hash: key_hash.clone(),
        name: req.name.clone(),
        resource_id: req.resource_id.clone(),
    };

    // Replicate to cluster
    let operation = WriteOp::CreateApiKey(api_key_record.clone());
    replicate_write(Arc::clone(&state), operation)
        .await
        .map_err(replication_error)?;

    // Apply locally after successful replication
    state
        .db
        .put_api_key(&api_key_record)
        .map_err(|e| ApiError::internal(format!("Failed to store API key: {}", e)))?;

    tracing::debug!(key_id = %api_key_record.id, name = %req.name, "Created API key");

    Ok(JSend::success(CreateApiKeyResponse {
        expires_at: api_key_record.expires_at.map(|t| t.to_rfc3339()),
        id: api_key_record.id,
        key,
        name: api_key_record.name,
        resource_id: api_key_record.resource_id,
    }))
}

pub async fn validate_api_key(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<Json<JSend<ApiKeyResponse>>, ApiError> {
    match api_key::validate(&state.db, &key) {
        Ok(Some(api_key_record)) => Ok(JSend::success(api_key_to_response(&api_key_record))),
        Ok(None) => Err(ApiError::not_found("API key not found or expired")),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

pub async fn revoke_api_key(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<Json<JSend<()>>, ApiError> {
    // Check if we're the leader (in cluster mode) - forward to leader if not
    if !state.config.is_single_node() {
        let cluster = state.cluster.read().await;
        if cluster.role != Role::Leader {
            drop(cluster);
            let path = format!("/api-keys/{}", key);
            forward_delete_to_leader(&state, &path).await?;
            return Ok(JSend::success(()));
        }
    }

    // Check if API key exists first
    let key_hash = hash_key(&key);
    match state.db.get_api_key(&key_hash) {
        Ok(Some(_)) => {}
        Ok(None) => return Err(ApiError::not_found("API key not found")),
        Err(e) => return Err(ApiError::internal(e.to_string())),
    }

    // Replicate revocation to cluster
    let operation = WriteOp::RevokeApiKey {
        key_id: key_hash.clone(),
    };
    replicate_write(Arc::clone(&state), operation)
        .await
        .map_err(replication_error)?;

    // Apply locally after successful replication
    state
        .db
        .delete_api_key(&key_hash)
        .map_err(|e| ApiError::internal(format!("Failed to delete API key: {}", e)))?;

    tracing::debug!(key_hash = %key_hash, "Revoked API key");
    Ok(JSend::success(()))
}

pub async fn list_api_keys(
    State(state): State<Arc<AppState>>,
    Path(resource_id): Path<String>,
) -> Result<Json<JSend<Vec<ApiKeyResponse>>>, ApiError> {
    match api_key::list_by_resource(&state.db, &resource_id) {
        Ok(keys) => Ok(JSend::success(
            keys.iter().map(api_key_to_response).collect(),
        )),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

// ============================================================================
// Utility handlers
// ============================================================================

pub async fn parse_user_agent_handler(
    Json(req): Json<ParseUserAgentRequest>,
) -> Json<JSend<DeviceInfoResponse>> {
    let device_info = parse_user_agent(&req.user_agent);
    JSend::success(DeviceInfoResponse {
        browser: device_info.browser,
        browser_version: device_info.browser_version,
        kind: format!("{:?}", device_info.kind),
        os: device_info.os,
        os_version: device_info.os_version,
    })
}

// ============================================================================
// Health and cluster handlers
// ============================================================================

pub async fn health(State(state): State<Arc<AppState>>) -> Json<JSend<HealthResponse>> {
    JSend::success(HealthResponse {
        node_id: state.config.node.id.clone(),
        status: "healthy".to_string(),
    })
}

pub async fn cluster_status(
    State(state): State<Arc<AppState>>,
) -> Json<JSend<ClusterStatusResponse>> {
    let cluster = state.cluster.read().await;

    JSend::success(ClusterStatusResponse {
        cluster_size: cluster.cluster_size(),
        node_id: state.config.node.id.clone(),
        peers: cluster
            .peer_states
            .iter()
            .map(|(id, peer)| PeerStatus {
                id: id.clone(),
                sequence: peer.sequence,
                status: peer.status.clone(),
            })
            .collect(),
        quorum: cluster.quorum_size(),
        role: format!("{:?}", cluster.role),
        sequence: cluster.last_applied_sequence,
        term: cluster.current_term,
    })
}

/// Purge all data from this node (for testing only)
/// This does NOT replicate - call on each node separately or use the cluster endpoint
pub async fn admin_purge(
    State(state): State<Arc<AppState>>,
) -> Result<Json<JSend<PurgeResponse>>, ApiError> {
    match state.db.purge_all() {
        Ok(stats) => {
            tracing::warn!(
                sessions = stats.sessions,
                api_keys = stats.api_keys,
                replication_entries = stats.replication_entries,
                "Purged all data"
            );
            Ok(JSend::success(PurgeResponse {
                api_keys_deleted: stats.api_keys,
                replication_entries_deleted: stats.replication_entries,
                sessions_deleted: stats.sessions,
            }))
        }
        Err(e) => Err(ApiError::internal(format!("Failed to purge data: {}", e))),
    }
}

// ============================================================================
// Internal replication handler (called by leader to replicate to followers)
// ============================================================================

pub async fn internal_replicate(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ReplicateRequest>,
) -> Result<Json<ReplicateResponse>, ApiError> {
    let mut cluster = state.cluster.write().await;

    // Verify term - reject if our term is higher
    if req.term < cluster.current_term {
        return Ok(Json(ReplicateResponse {
            success: false,
            sequence: cluster.last_applied_sequence,
        }));
    }

    // Update term and recognize leader if needed
    if req.term > cluster.current_term {
        cluster.become_follower(req.term, Some(req.leader_id.clone()));
    }

    cluster.update_heartbeat();
    drop(cluster);

    // Apply the operation locally
    match apply_write_op(&state.db, &req.operation) {
        Ok(()) => {
            // Record in replication log
            let write = ReplicatedWrite {
                sequence: req.sequence,
                operation: req.operation,
                timestamp: Utc::now(),
            };

            if let Err(e) = state.db.append_replication_log(&write) {
                tracing::error!(error = %e, "Failed to append to replication log");
                return Err(ApiError::internal(format!(
                    "Failed to append to replication log: {}",
                    e
                )));
            }

            // Update applied sequence
            let mut cluster = state.cluster.write().await;
            cluster.last_applied_sequence = req.sequence;

            tracing::debug!(sequence = req.sequence, "Applied replicated write");

            Ok(Json(ReplicateResponse {
                success: true,
                sequence: req.sequence,
            }))
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to apply replicated write");
            Err(ApiError::internal(format!("Failed to apply write: {}", e)))
        }
    }
}

/// Apply a write operation to the local database
fn apply_write_op(
    db: &crate::storage::Database,
    op: &WriteOp,
) -> Result<(), crate::storage::DatabaseError> {
    match op {
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
    }
    Ok(())
}

// ============================================================================
// Internal heartbeat handler (called by leader to maintain leadership)
// ============================================================================

pub async fn internal_heartbeat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<HeartbeatRequest>,
) -> Json<HeartbeatResponse> {
    let mut cluster = state.cluster.write().await;

    // If term is less than ours, reject
    if req.term < cluster.current_term {
        return Json(HeartbeatResponse {
            term: cluster.current_term,
            success: false,
            sequence: cluster.last_applied_sequence,
        });
    }

    // Accept the heartbeat - step down if we thought we were leader
    if req.term > cluster.current_term || cluster.role != Role::Follower {
        cluster.become_follower(req.term, Some(req.leader_id.clone()));
    }

    cluster.leader_id = Some(req.leader_id);
    if let Some(addr) = req.leader_address {
        cluster.leader_address = Some(addr);
    }
    cluster.update_heartbeat();

    let sequence = cluster.last_applied_sequence;
    let term = cluster.current_term;

    Json(HeartbeatResponse {
        term,
        success: true,
        sequence,
    })
}

// ============================================================================
// Internal vote handler (called during leader election)
// ============================================================================

pub async fn internal_vote(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VoteRequest>,
) -> Json<VoteResponse> {
    let mut cluster = state.cluster.write().await;

    // If candidate's term is less than ours, reject
    if req.term < cluster.current_term {
        return Json(VoteResponse {
            term: cluster.current_term,
            vote_granted: false,
        });
    }

    // If candidate's term is greater, become follower
    if req.term > cluster.current_term {
        cluster.become_follower(req.term, None);
    }

    // Grant vote if we haven't voted yet in this term (or already voted for this candidate)
    let dominated_log = req.last_sequence >= cluster.last_applied_sequence;
    let grant = dominated_log
        && (cluster.voted_for.is_none() || cluster.voted_for.as_deref() == Some(&req.candidate_id));

    if grant {
        cluster.voted_for = Some(req.candidate_id.clone());
        cluster.update_heartbeat(); // Reset election timeout

        if let Err(e) = cluster.persist(&state.db, &state.config.node.id) {
            tracing::warn!(error = %e, "Failed to persist vote");
        }

        tracing::debug!(candidate = %req.candidate_id, term = req.term, "Granted vote");
    }

    Json(VoteResponse {
        term: cluster.current_term,
        vote_granted: grant,
    })
}

// ============================================================================
// Helper functions
// ============================================================================

fn session_to_response(session: &SessionToken) -> SessionResponse {
    SessionResponse {
        token: session.id.clone(),
        resource_id: session.resource_id.clone(),
        created_at: session.created_at.to_rfc3339(),
        expires_at: session.expires_at.to_rfc3339(),
        device_info: DeviceInfoResponse {
            kind: format!("{:?}", session.device_info.kind),
            os: session.device_info.os.clone(),
            os_version: session.device_info.os_version.clone(),
            browser: session.device_info.browser.clone(),
            browser_version: session.device_info.browser_version.clone(),
        },
    }
}

fn api_key_to_response(api_key: &ApiKeyModel) -> ApiKeyResponse {
    ApiKeyResponse {
        created_at: api_key.created_at.to_rfc3339(),
        expires_at: api_key.expires_at.map(|t| t.to_rfc3339()),
        id: api_key.id.clone(),
        name: api_key.name.clone(),
        resource_id: api_key.resource_id.clone(),
    }
}
