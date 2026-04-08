//! qtss-auth — minimal RBAC + session model.
//!
//! Responsibilities:
//!   * Hash and verify passwords with argon2id.
//!   * Resolve a user's role set and check it against a required permission.
//!   * Issue and validate session tokens persisted in the `sessions` table.
//!
//! Permissions are a static `Role -> &[Permission]` map kept in `roles.rs`
//! — editing them is a code review, not an UPDATE statement. Roles
//! themselves are seeded by migration 0015.

mod error;
mod password;
mod roles;
mod session;
mod store;

#[cfg(test)]
mod tests;

pub use error::{AuthError, AuthResult};
pub use password::{hash_password, verify_password};
pub use roles::{permissions_for, Permission, RoleName};
pub use session::{Session, SessionToken};
pub use store::{AuthStore, MemoryAuthStore, PgAuthStore, UserRecord};
