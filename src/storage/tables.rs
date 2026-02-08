use redb::TableDefinition;

/// Session tokens: token -> SessionToken (bincode)
pub const SESSIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("sessions");

/// API keys: key_hash -> ApiKey (bincode)
pub const API_KEYS: TableDefinition<&str, &[u8]> = TableDefinition::new("api_keys");

/// Replication log: sequence_number -> ReplicatedWrite (bincode)
pub const REPLICATION_LOG: TableDefinition<u64, &[u8]> = TableDefinition::new("replication_log");

/// Node state: "state" -> NodeState (bincode)
pub const NODE_META: TableDefinition<&str, &[u8]> = TableDefinition::new("node_meta");

/// Secondary index: resource_id -> Vec<key_hash> (for listing API keys by resource)
pub const RESOURCE_API_KEYS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("resource_api_keys");

/// Secondary index: resource_id -> Vec<token> (for listing sessions by resource)
pub const RESOURCE_SESSIONS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("resource_sessions");

/// Secondary index: session UUID id -> token (for revoking sessions by ID)
pub const SESSION_IDS: TableDefinition<&str, &str> = TableDefinition::new("session_ids");

/// Secondary index: API key UUID id -> key_hash (for revoking API keys by ID)
pub const API_KEY_IDS: TableDefinition<&str, &str> = TableDefinition::new("api_key_ids");
