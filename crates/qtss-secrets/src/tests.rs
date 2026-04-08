use crate::cipher::{open, seal};
use crate::error::SecretError;
use crate::kek::{KekProvider, StaticKek};
use crate::store::{MemorySecretStore, SecretStore};
use std::sync::Arc;

fn test_kek() -> Arc<dyn KekProvider> {
    Arc::new(StaticKek::new(1, [7u8; 32]))
}

#[test]
fn seal_then_open_round_trip() {
    let kek = test_kek();
    let plaintext = b"binance:api_secret_xyz";
    let sealed = seal(kek.as_ref(), plaintext).unwrap();
    let opened = open(kek.as_ref(), &sealed).unwrap();
    assert_eq!(opened, plaintext);
}

#[test]
fn each_seal_uses_fresh_dek_and_nonce() {
    let kek = test_kek();
    let pt = b"same plaintext";
    let a = seal(kek.as_ref(), pt).unwrap();
    let b = seal(kek.as_ref(), pt).unwrap();
    assert_ne!(a.ciphertext, b.ciphertext, "ciphertext must differ");
    assert_ne!(a.nonce, b.nonce, "nonce must be fresh per call");
    assert_ne!(a.wrapped_dek, b.wrapped_dek, "DEK must be fresh per call");
}

#[test]
fn unknown_kek_version_refuses_to_unwrap() {
    let kek_v1 = StaticKek::new(1, [7u8; 32]);
    let sealed = seal(&kek_v1, b"hello").unwrap();
    // Verifier holds a different version — must refuse rather than silently
    // returning garbage. This is what protects us after a KEK rotation.
    let kek_v2 = StaticKek::new(2, [9u8; 32]);
    let err = open(&kek_v2, &sealed).unwrap_err();
    assert!(matches!(err, SecretError::UnknownKekVersion(1)));
}

#[tokio::test]
async fn memory_store_round_trip_and_duplicate_detection() {
    let store = MemorySecretStore::new(test_kek());
    store
        .put("binance.api_key", Some("live key"), b"AKIA...", "alice")
        .await
        .unwrap();
    let got = store.get("binance.api_key").await.unwrap();
    assert_eq!(got, b"AKIA...");

    let dup = store
        .put("binance.api_key", None, b"other", "alice")
        .await
        .unwrap_err();
    assert!(matches!(dup, SecretError::AlreadyExists(_)));

    let listed = store.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "binance.api_key");
}

#[tokio::test]
async fn missing_secret_returns_not_found() {
    let store = MemorySecretStore::new(test_kek());
    let err = store.get("does.not.exist").await.unwrap_err();
    assert!(matches!(err, SecretError::NotFound(_)));
}
