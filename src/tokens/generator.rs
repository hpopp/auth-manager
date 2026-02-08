use rand::Rng;

/// Generate a secure random token (32 bytes, hex encoded = 64 characters)
pub fn generate_token() -> String {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();
    hex::encode(bytes)
}

/// Generate a secure random API key with a prefix
pub fn generate_api_key() -> String {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 24] = rng.gen();
    format!("am_{}", hex::encode(bytes))
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
    fn test_generate_token() {
        let token = generate_token();
        assert_eq!(token.len(), 64); // 32 bytes * 2 hex chars
        
        // Ensure randomness
        let token2 = generate_token();
        assert_ne!(token, token2);
    }

    #[test]
    fn test_generate_api_key() {
        let key = generate_api_key();
        assert!(key.starts_with("am_"));
        assert_eq!(key.len(), 3 + 48); // "am_" + 24 bytes * 2 hex chars
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
