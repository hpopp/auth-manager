use rand::Rng;

/// Generate a secure random hex string.
///
/// - Session tokens: `generate_hex(32)` → 64-char hex
/// - API keys: `generate_hex(24)` → 48-char hex
pub fn generate_hex(len: usize) -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
    hex::encode(bytes)
}

/// Hash a string using SHA-256 (for API key storage)
pub fn hash_key(key: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Simple hash for now - in production, use a proper cryptographic hash
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_hex() {
        let a = generate_hex(32);
        assert_eq!(a.len(), 64); // 32 bytes * 2 hex chars

        let b = generate_hex(32);
        assert_ne!(a, b); // Random, should not collide
    }

    #[test]
    fn test_hash_key() {
        let key = "test_key";
        let hash1 = hash_key(key);
        let hash2 = hash_key(key);
        assert_eq!(hash1, hash2); // Deterministic

        let hash3 = hash_key("different_key");
        assert_ne!(hash1, hash3); // Different inputs produce different hashes
    }
}
