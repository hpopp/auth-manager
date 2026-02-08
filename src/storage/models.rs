use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Device kind detected from User-Agent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DeviceKind {
    Bot,
    Desktop,
    Mobile,
    Tablet,
    #[default]
    Unknown,
}

/// Information about the device that created a session
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceInfo {
    pub browser: Option<String>,
    pub browser_version: Option<String>,
    pub kind: DeviceKind,
    pub os: Option<String>,
    pub os_version: Option<String>,
    pub raw_user_agent: String,
}

/// A session token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionToken {
    /// When the token was created
    pub created_at: DateTime<Utc>,
    /// Information about the device that created this session
    pub device_info: DeviceInfo,
    /// When the token expires
    pub expires_at: DateTime<Utc>,
    /// Non-secret UUID identifier (used for listing, revoking)
    pub id: String,
    /// The actor (user, service, device, etc.)
    pub subject_id: String,
    /// Opaque secret token (32-byte hex, used for verification)
    pub token: String,
}

/// An API key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// When the key was created
    pub created_at: DateTime<Utc>,
    /// Optional description of the key's purpose
    pub description: Option<String>,
    /// When the key expires (optional)
    pub expires_at: Option<DateTime<Utc>>,
    /// Key identifier (used for lookups after hashing)
    pub id: String,
    /// Hash of the actual key (we don't store the plaintext)
    pub key_hash: String,
    /// Human-readable name for the key
    pub name: String,
    /// The actor (user, service, device, etc.)
    pub subject_id: String,
    /// Permission scopes granted to this key
    pub scopes: Vec<String>,
}

/// A write operation for replication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicatedWrite {
    /// The operation being replicated
    pub operation: WriteOp,
    /// Monotonic, gapless sequence number
    pub sequence: u64,
    /// When the write occurred
    pub timestamp: DateTime<Utc>,
}

/// Types of write operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WriteOp {
    CreateApiKey(ApiKey),
    CreateSession(SessionToken),
    RevokeApiKey { key_id: String },
    RevokeSession { token_id: String },
    UpdateApiKey {
        key_hash: String,
        description: Option<Option<String>>,
        name: Option<String>,
        scopes: Option<Vec<String>>,
    },
}

/// Persistent node state (stored in redb)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeState {
    pub current_term: u64,
    pub last_applied_sequence: u64,
    pub node_id: String,
    pub voted_for: Option<String>,
}

impl NodeState {
    pub fn new(node_id: String) -> Self {
        Self {
            node_id,
            current_term: 0,
            voted_for: None,
            last_applied_sequence: 0,
        }
    }
}
