//! KEK (master key) provider abstraction. Production deployments plug in
//! a KMS-backed implementation; tests and bootstrap use `StaticKek`.

use crate::error::{SecretError, SecretResult};

pub trait KekProvider: Send + Sync {
    /// The currently active KEK version. New writes use this version.
    fn current_version(&self) -> i32;
    /// Resolve a 32-byte key for the given version. Failing versions are
    /// surfaced as `UnknownKekVersion` so the caller can refuse to operate
    /// on legacy ciphertexts after a rotation purge.
    fn key_for(&self, version: i32) -> SecretResult<[u8; 32]>;
}

/// In-process KEK held in memory. Acceptable for single-node bootstrap;
/// real deployments should swap this for a KMS-backed provider.
pub struct StaticKek {
    version: i32,
    key: [u8; 32],
}

impl StaticKek {
    pub fn new(version: i32, key: [u8; 32]) -> Self {
        Self { version, key }
    }
}

impl KekProvider for StaticKek {
    fn current_version(&self) -> i32 {
        self.version
    }
    fn key_for(&self, version: i32) -> SecretResult<[u8; 32]> {
        if version == self.version {
            Ok(self.key)
        } else {
            Err(SecretError::UnknownKekVersion(version))
        }
    }
}
