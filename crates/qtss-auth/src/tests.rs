use crate::error::AuthError;
use crate::password::{hash_password, verify_password};
use crate::roles::{permissions_for, Permission, RoleName};
use crate::store::{AuthStore, MemoryAuthStore};
use chrono::Duration;

#[test]
fn argon2_hash_round_trip() {
    let h = hash_password("correct horse battery staple").unwrap();
    verify_password("correct horse battery staple", &h).unwrap();
    let bad = verify_password("wrong", &h).unwrap_err();
    assert!(matches!(bad, AuthError::InvalidCredentials));
}

#[test]
fn role_permission_table_matches_expectations() {
    assert!(permissions_for(RoleName::Admin).contains(&Permission::UserManage));
    assert!(permissions_for(RoleName::Trader).contains(&Permission::IntentApprove));
    assert!(!permissions_for(RoleName::Trader).contains(&Permission::UserManage));
    assert!(permissions_for(RoleName::Viewer).contains(&Permission::ViewDashboards));
    assert!(!permissions_for(RoleName::Viewer).contains(&Permission::ConfigWrite));
}

#[tokio::test]
async fn create_login_validate_revoke_round_trip() {
    let store = MemoryAuthStore::new();
    store
        .create_user("alice", Some("a@example.com"), "hunter2", &[RoleName::Admin])
        .await
        .unwrap();

    let (user, session) = store
        .login("alice", "hunter2", Duration::minutes(30))
        .await
        .unwrap();
    assert_eq!(user.username, "alice");
    assert!(user.has_permission(Permission::ConfigWrite));

    let validated = store.validate_session(session.token).await.unwrap();
    assert_eq!(validated.id, user.id);

    store.revoke_session(session.token).await.unwrap();
    let err = store.validate_session(session.token).await.unwrap_err();
    assert!(matches!(err, AuthError::SessionInvalid));
}

#[tokio::test]
async fn login_with_wrong_password_is_rejected() {
    let store = MemoryAuthStore::new();
    store
        .create_user("bob", None, "right", &[RoleName::Trader])
        .await
        .unwrap();
    let err = store
        .login("bob", "wrong", Duration::minutes(5))
        .await
        .unwrap_err();
    assert!(matches!(err, AuthError::InvalidCredentials));
}

#[tokio::test]
async fn unknown_user_returns_user_not_found() {
    let store = MemoryAuthStore::new();
    let err = store
        .login("nobody", "x", Duration::minutes(5))
        .await
        .unwrap_err();
    assert!(matches!(err, AuthError::UserNotFound(_)));
}
