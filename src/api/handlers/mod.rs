mod admin;
mod api_keys;
mod sessions;

use serde::Deserialize;

use crate::api::response::ApiError;

/// Query parameters for list endpoints
#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
    pub subject_id: Option<String>,
}

impl ListParams {
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
pub use sessions::{create_session, get_session, list_sessions, revoke_session, validate_session};

/// Map a MusterError to an ApiError
fn replication_error(e: muster::MusterError) -> ApiError {
    match e {
        muster::MusterError::NotLeader { .. } => {
            ApiError::unavailable("No leader available — retry shortly")
        }
        muster::MusterError::NoQuorum => {
            ApiError::unavailable("Failed to reach quorum for replication")
        }
        _ => ApiError::internal(e.to_string()),
    }
}
