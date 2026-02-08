use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, error};

use crate::tokens::{api_key, session};
use crate::AppState;

/// Start the background expiration cleaner task
pub fn start_expiration_cleaner(state: Arc<AppState>) -> JoinHandle<()> {
    let interval = Duration::from_secs(state.config.tokens.cleanup_interval_seconds);

    tokio::spawn(async move {
        let mut interval_timer = tokio::time::interval(interval);

        loop {
            interval_timer.tick().await;

            debug!("Running expiration cleanup");

            // Clean up expired sessions
            match session::cleanup_expired(&state.db) {
                Ok(count) => {
                    if count > 0 {
                        debug!(sessions_cleaned = count, "Expired sessions cleaned");
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to clean up expired sessions");
                }
            }

            // Clean up expired API keys
            match api_key::cleanup_expired(&state.db) {
                Ok(count) => {
                    if count > 0 {
                        debug!(api_keys_cleaned = count, "Expired API keys cleaned");
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to clean up expired API keys");
                }
            }
        }
    })
}
