//! End-to-end integration tests

use tempfile::TempDir;

// Helper to create a test database
fn setup_test_db() -> (auth_manager::storage::Database, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db = auth_manager::storage::Database::open(temp_dir.path()).unwrap();
    (db, temp_dir)
}

#[tokio::test]
async fn test_session_lifecycle() {
    let (db, _temp) = setup_test_db();

    // Create a session
    let device_info = auth_manager::storage::models::DeviceInfo::default();
    let session =
        auth_manager::tokens::session::create(&db, "user-123", device_info, 3600).unwrap();

    // Validate it exists
    let validated = auth_manager::tokens::session::validate(&db, &session.id).unwrap();
    assert!(validated.is_some());
    assert_eq!(validated.unwrap().resource_id, "user-123");

    // Revoke it
    let revoked = auth_manager::tokens::session::revoke(&db, &session.id).unwrap();
    assert!(revoked);

    // Verify it's gone
    let validated = auth_manager::tokens::session::validate(&db, &session.id).unwrap();
    assert!(validated.is_none());
}

#[tokio::test]
async fn test_api_key_lifecycle() {
    let (db, _temp) = setup_test_db();

    // Create an API key
    let (key, api_key) =
        auth_manager::tokens::api_key::create(&db, "Test API Key", "resource-123", Some(30))
            .unwrap();
    assert!(key.starts_with("am_"));
    assert_eq!(api_key.name, "Test API Key");

    // Validate it exists
    let validated = auth_manager::tokens::api_key::validate(&db, &key).unwrap();
    assert!(validated.is_some());

    // Revoke it
    let revoked = auth_manager::tokens::api_key::revoke(&db, &key).unwrap();
    assert!(revoked);

    // Verify it's gone
    let validated = auth_manager::tokens::api_key::validate(&db, &key).unwrap();
    assert!(validated.is_none());
}

#[tokio::test]
async fn test_multiple_sessions_per_resource() {
    let (db, _temp) = setup_test_db();

    // Create multiple sessions for the same resource
    let device_info = auth_manager::storage::models::DeviceInfo::default();
    let s1 =
        auth_manager::tokens::session::create(&db, "user-456", device_info.clone(), 3600).unwrap();
    let s2 =
        auth_manager::tokens::session::create(&db, "user-456", device_info.clone(), 3600).unwrap();
    let s3 =
        auth_manager::tokens::session::create(&db, "user-789", device_info.clone(), 3600).unwrap();

    // List sessions by resource
    let sessions = auth_manager::tokens::session::list_by_resource(&db, "user-456").unwrap();
    assert_eq!(sessions.len(), 2);

    let sessions = auth_manager::tokens::session::list_by_resource(&db, "user-789").unwrap();
    assert_eq!(sessions.len(), 1);

    // Revoke one session
    auth_manager::tokens::session::revoke(&db, &s1.id).unwrap();

    // Verify only one remains
    let sessions = auth_manager::tokens::session::list_by_resource(&db, "user-456").unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, s2.id);
}
