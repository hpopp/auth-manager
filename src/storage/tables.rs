use redb::TableDefinition;

/// Session tokens: token -> SessionToken (msgpack)
pub const SESSIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("sessions");

/// API keys: key_hash -> ApiKey (msgpack)
pub const API_KEYS: TableDefinition<&str, &[u8]> = TableDefinition::new("api_keys");

/// Secondary index: subject_id -> Vec<key_hash> (for listing API keys by subject)
pub const SUBJECT_API_KEYS: TableDefinition<&str, &[u8]> = TableDefinition::new("subject_api_keys");

/// Secondary index: subject_id -> Vec<token> (for listing sessions by subject)
pub const SUBJECT_SESSIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("subject_sessions");

/// Secondary index: session UUID id -> token (for revoking sessions by ID)
pub const SESSION_IDS: TableDefinition<&str, &str> = TableDefinition::new("session_ids");

/// Secondary index: API key UUID id -> key_hash (for revoking API keys by ID)
pub const API_KEY_IDS: TableDefinition<&str, &str> = TableDefinition::new("api_key_ids");
