//! End-to-end integration tests

use auth_manager::storage::models::{DeviceInfo, SessionToken};
use auth_manager::storage::Database;
use chrono::Utc;
use tempfile::TempDir;

fn setup_db() -> (Database, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db = Database::open(temp_dir.path()).unwrap();
    (db, temp_dir)
}

fn make_session(id: &str, subject: &str) -> SessionToken {
    let now = Utc::now();
    SessionToken {
        created_at: now,
        device_info: DeviceInfo::default(),
        expires_at: now + chrono::Duration::hours(24),
        id: id.to_string(),
        ip_address: None,
        last_used_at: None,
        subject_id: subject.to_string(),
        token: format!("tok_{id}"),
    }
}

#[tokio::test]
async fn test_session_lifecycle() {
    let (db, _temp) = setup_db();

    // Store a session
    let session = make_session("s1", "user-123");
    db.put_session(&session).unwrap();

    // Validate it exists (by secret token)
    let validated = auth_manager::tokens::session::validate(&db, &session.token).unwrap();
    assert!(validated.is_some());
    assert_eq!(validated.unwrap().subject_id, "user-123");

    // Revoke it (by secret token)
    assert!(db.delete_session(&session.token).unwrap());

    // Verify it's gone
    let validated = auth_manager::tokens::session::validate(&db, &session.token).unwrap();
    assert!(validated.is_none());
}

#[tokio::test]
async fn test_api_key_lifecycle() {
    let (db, _temp) = setup_db();

    // Store an API key
    let now = Utc::now();
    let api_key = auth_manager::storage::models::ApiKey {
        created_at: now,
        description: None,
        expires_at: Some(now + chrono::Duration::days(30)),
        id: "k1".to_string(),
        key_hash: "hash_k1".to_string(),
        last_used_at: None,
        name: "Test API Key".to_string(),
        subject_id: "subject-123".to_string(),
        scopes: vec![],
        updated_at: Some(now),
    };
    db.put_api_key(&api_key).unwrap();

    // Validate it exists (by hash lookup)
    let fetched = db.get_api_key(&api_key.key_hash).unwrap();
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().name, "Test API Key");

    // Revoke it
    assert!(db.delete_api_key(&api_key.key_hash).unwrap());

    // Verify it's gone
    let fetched = db.get_api_key(&api_key.key_hash).unwrap();
    assert!(fetched.is_none());
}

#[tokio::test]
async fn test_multiple_sessions_per_subject() {
    let (db, _temp) = setup_db();

    // Create multiple sessions for the same subject
    let s1 = make_session("s1", "user-456");
    let s2 = make_session("s2", "user-456");
    let s3 = make_session("s3", "user-789");
    db.put_session(&s1).unwrap();
    db.put_session(&s2).unwrap();
    db.put_session(&s3).unwrap();

    // List sessions by subject
    let sessions = auth_manager::tokens::session::list_by_subject(&db, "user-456").unwrap();
    assert_eq!(sessions.len(), 2);

    let sessions = auth_manager::tokens::session::list_by_subject(&db, "user-789").unwrap();
    assert_eq!(sessions.len(), 1);

    // Revoke one session (by token)
    db.delete_session(&s1.token).unwrap();

    // Verify only one remains
    let sessions = auth_manager::tokens::session::list_by_subject(&db, "user-456").unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, s2.id);
}
