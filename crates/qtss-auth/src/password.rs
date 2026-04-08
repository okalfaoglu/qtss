//! Argon2id password hashing. We accept the default Argon2 parameters
//! from the `argon2` crate, which already match OWASP guidance, and rely
//! on PHC string output so the verifier picks up params from the hash
//! itself — future tuning won't break old credentials.

use crate::error::{AuthError, AuthResult};
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

pub fn hash_password(password: &str) -> AuthResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AuthError::Hash(e.to_string()))
}

pub fn verify_password(password: &str, phc_hash: &str) -> AuthResult<()> {
    let parsed = PasswordHash::new(phc_hash).map_err(|e| AuthError::Hash(e.to_string()))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AuthError::InvalidCredentials)
}
