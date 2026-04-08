//! Envelope encryption primitives. AES-256-GCM throughout — same algo for
//! both DEK wrapping and payload sealing keeps the dependency surface small.

use crate::error::{SecretError, SecretResult};
use crate::kek::KekProvider;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;

/// What gets persisted in `secrets_vault`. All fields are opaque bytes
/// with no hint about the underlying secret.
#[derive(Debug, Clone)]
pub struct SealedSecret {
    pub wrapped_dek: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub kek_version: i32,
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    rand::thread_rng().fill_bytes(&mut buf);
    buf
}

fn aes_seal(key: &[u8; 32], plaintext: &[u8]) -> SecretResult<(Vec<u8>, Vec<u8>)> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce_bytes = random_bytes::<12>();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| SecretError::Crypto(e.to_string()))?;
    Ok((ct, nonce_bytes.to_vec()))
}

fn aes_open(key: &[u8; 32], nonce: &[u8], ciphertext: &[u8]) -> SecretResult<Vec<u8>> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| SecretError::Crypto(e.to_string()))
}

/// Generate a fresh DEK, encrypt the plaintext with it, then wrap the DEK
/// with the current KEK. Result is everything you need to persist.
pub fn seal(kek: &dyn KekProvider, plaintext: &[u8]) -> SecretResult<SealedSecret> {
    let dek = random_bytes::<32>();
    let (ciphertext, nonce) = aes_seal(&dek, plaintext)?;

    let version = kek.current_version();
    let master = kek.key_for(version)?;
    // Wrap the DEK using its own nonce. We embed the wrap nonce as the
    // first 12 bytes of `wrapped_dek` so unwrapping is self-contained.
    let (wrapped_body, wrap_nonce) = aes_seal(&master, &dek)?;
    let mut wrapped_dek = Vec::with_capacity(12 + wrapped_body.len());
    wrapped_dek.extend_from_slice(&wrap_nonce);
    wrapped_dek.extend_from_slice(&wrapped_body);

    Ok(SealedSecret {
        wrapped_dek,
        ciphertext,
        nonce,
        kek_version: version,
    })
}

/// Inverse of [`seal`]. Looks up the KEK version that wrapped this DEK,
/// unwraps it, and decrypts the payload.
pub fn open(kek: &dyn KekProvider, sealed: &SealedSecret) -> SecretResult<Vec<u8>> {
    if sealed.wrapped_dek.len() < 12 {
        return Err(SecretError::Crypto("wrapped_dek too short".into()));
    }
    let (wrap_nonce, wrap_body) = sealed.wrapped_dek.split_at(12);
    let master = kek.key_for(sealed.kek_version)?;
    let dek_vec = aes_open(&master, wrap_nonce, wrap_body)?;
    if dek_vec.len() != 32 {
        return Err(SecretError::Crypto("unwrapped DEK has wrong length".into()));
    }
    let mut dek = [0u8; 32];
    dek.copy_from_slice(&dek_vec);
    aes_open(&dek, &sealed.nonce, &sealed.ciphertext)
}
