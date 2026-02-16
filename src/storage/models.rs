use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    /// IP address of the client that created this session
    #[serde(default)]
    pub ip_address: Option<String>,
    /// When the token was last used for validation
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
    /// Arbitrary client-supplied key-value data (e.g. user profile info)
    #[serde(default)]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
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
    /// When the key was last used for validation
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
    /// Human-readable name for the key
    pub name: String,
    /// The actor (user, service, device, etc.)
    pub subject_id: String,
    /// Permission scopes granted to this key
    pub scopes: Vec<String>,
    /// When the key was last updated
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

impl SessionToken {
    /// Pure check: is this session expired at the given time?
    pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        self.expires_at < now
    }
}

impl ApiKey {
    /// Pure check: is this API key expired at the given time?
    /// Returns false if no expiration is set.
    pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.map(|exp| exp < now).unwrap_or(false)
    }
}

/// Types of write operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WriteOp {
    CreateApiKey(ApiKey),
    CreateSession(SessionToken),
    RevokeApiKey {
        key_id: String,
    },
    RevokeSession {
        token_id: String,
    },
    UpdateApiKey {
        key_hash: String,
        description: Option<Option<String>>,
        name: Option<String>,
        scopes: Option<Vec<String>>,
    },
}
