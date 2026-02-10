mod admin;
mod api_keys;
mod internal;
mod sessions;

use serde::Deserialize;

use crate::api::response::ApiError;
use crate::cluster::ReplicationError;

/// Shared pagination query parameters for list endpoints
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

impl PaginationParams {
    pub fn validate(&self) -> Result<(), ApiError> {
        if self.limit == 0 {
            return Err(ApiError::bad_request("limit must be greater than 0"));
        }
        Ok(())
    }
}

fn default_limit() -> u32 {
    20
}

pub use admin::{admin_purge, cluster_status, health, parse_user_agent_handler};
pub use api_keys::{
    create_api_key, get_api_key, list_api_keys, revoke_api_key, update_api_key, validate_api_key,
};
pub use internal::{internal_heartbeat, internal_replicate, internal_sync, internal_vote};
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
