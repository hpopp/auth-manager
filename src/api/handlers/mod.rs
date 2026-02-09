mod admin;
mod api_keys;
mod internal;
mod sessions;

use crate::api::response::ApiError;
use crate::cluster::ReplicationError;

pub use admin::{admin_purge, cluster_status, health, parse_user_agent_handler};
pub use api_keys::{
    create_api_key, get_api_key, list_api_keys, revoke_api_key, update_api_key, validate_api_key,
};
pub use internal::{internal_heartbeat, internal_replicate, internal_vote};
pub use sessions::{create_session, get_session, list_sessions, revoke_session, validate_session};

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
