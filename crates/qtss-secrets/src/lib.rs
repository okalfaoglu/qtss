//! qtss-secrets — envelope-encrypted secret storage.
//!
//! Two layers of encryption:
//!   1. **DEK** (Data Encryption Key) — fresh AES-256-GCM key per secret.
//!      Encrypts the actual secret payload.
//!   2. **KEK** (Key Encryption Key, a.k.a. master key) — wraps the DEK.
//!      Lives outside Postgres (env var, KMS, file). Rotation = re-wrap
//!      every DEK with the new KEK and bump `kek_version`.
//!
//! Postgres only ever sees the wrapped DEK + ciphertext + nonce. A DB
//! dump on its own is useless without the KEK.

mod bootstrap;
mod cipher;
mod error;
mod kek;
mod reader;
mod store;

#[cfg(test)]
mod tests;

pub use bootstrap::{load_static_kek_from_env, KekBootstrapError};
pub use cipher::{seal, open, SealedSecret};
pub use error::{SecretError, SecretResult};
pub use kek::{KekProvider, StaticKek};
pub use reader::{SecretSource, VaultReader};
pub use store::{MemorySecretStore, PgSecretStore, SecretStore, StoredSecret};
