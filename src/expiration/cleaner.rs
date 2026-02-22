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
            run_cleanup(&state).await;
        }
    })
}

async fn run_cleanup(state: &AppState) {
    debug!("Running expiration cleanup");

    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || {
        let sessions = session::cleanup_expired(&db);
        let api_keys = api_key::cleanup_expired(&db);
        (sessions, api_keys)
    })
    .await;

    let (session_result, api_key_result) = match result {
        Ok(results) => results,
        Err(e) => {
            error!(error = %e, "Expiration cleanup task panicked");
            return;
        }
    };

    match session_result {
        Ok(count) if count > 0 => debug!(sessions_cleaned = count, "Expired sessions cleaned"),
        Err(e) => error!(error = %e, "Failed to clean up expired sessions"),
        _ => {}
    }

    match api_key_result {
        Ok(count) if count > 0 => debug!(api_keys_cleaned = count, "Expired API keys cleaned"),
        Err(e) => error!(error = %e, "Failed to clean up expired API keys"),
        _ => {}
    }
}
