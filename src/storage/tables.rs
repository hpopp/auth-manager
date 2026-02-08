use redb::TableDefinition;

/// Session tokens: token_id -> SessionToken (bincode)
pub const SESSIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("sessions");

/// API keys: key_hash -> ApiKey (bincode)
pub const API_KEYS: TableDefinition<&str, &[u8]> = TableDefinition::new("api_keys");

/// Replication log: sequence_number -> ReplicatedWrite (bincode)
pub const REPLICATION_LOG: TableDefinition<u64, &[u8]> = TableDefinition::new("replication_log");

/// Node state: "state" -> NodeState (bincode)
pub const NODE_META: TableDefinition<&str, &[u8]> = TableDefinition::new("node_meta");

/// Secondary index: resource_id -> Vec<key_hash> (for listing API keys by resource)
pub const RESOURCE_API_KEYS: TableDefinition<&str, &[u8]> = TableDefinition::new("resource_api_keys");

/// Secondary index: resource_id -> Vec<token_id> (for listing sessions by resource)
pub const RESOURCE_SESSIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("resource_sessions");
