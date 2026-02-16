//! auth-manager's state machine for muster cluster replication.

use serde::{Deserialize, Serialize};

use crate::storage::models::{ApiKey, SessionToken, WriteOp};
use crate::storage::Database;

/// The auth-manager state machine, replicated by muster.
pub struct AuthStateMachine {
    db: Database,
}

impl AuthStateMachine {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

/// Full state snapshot for syncing lagging followers.
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthSnapshot {
    pub api_keys: Vec<ApiKey>,
    pub sessions: Vec<SessionToken>,
}

impl muster::StateMachine for AuthStateMachine {
    type WriteOp = WriteOp;
    type Snapshot = AuthSnapshot;

    fn apply(&self, op: &WriteOp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match op {
            WriteOp::CreateSession(session) => {
                self.db.put_session(session)?;
            }
            WriteOp::RevokeSession { token_id } => {
                self.db.delete_session(token_id)?;
            }
            WriteOp::CreateApiKey(api_key) => {
                self.db.put_api_key(api_key)?;
            }
            WriteOp::RevokeApiKey { key_id } => {
                self.db.delete_api_key(key_id)?;
            }
            WriteOp::UpdateApiKey {
                key_hash,
                description,
                name,
                scopes,
            } => {
                self.db.update_api_key(
                    key_hash,
                    name.as_deref(),
                    description.as_ref().map(|d| d.as_deref()),
                    scopes.as_deref(),
                )?;
            }
        }
        Ok(())
    }

    fn snapshot(&self) -> Result<AuthSnapshot, Box<dyn std::error::Error + Send + Sync>> {
        let sessions = self.db.get_all_sessions()?;
        let api_keys = self.db.get_all_api_keys()?;
        Ok(AuthSnapshot { sessions, api_keys })
    }

    fn restore(
        &self,
        snapshot: AuthSnapshot,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for session in &snapshot.sessions {
            self.db.put_session(session)?;
        }
        for api_key in &snapshot.api_keys {
            self.db.put_api_key(api_key)?;
        }
        Ok(())
    }
}
