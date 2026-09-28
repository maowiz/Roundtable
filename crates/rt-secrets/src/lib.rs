//! Credential storage through Windows Credential Manager (KEY-01…06).
//!
//! Rules enforced here:
//! - A secret is only ever returned as [`Secret`], which zeroizes on drop and redacts in `Debug`.
//! - Nothing in this crate logs, serializes or formats the secret value.
//! - Callers outside the dispatch layer only get labels and status.

use rt_core::CredentialRef;
use std::fmt;
use zeroize::{Zeroize, ZeroizeOnDrop};

const SERVICE: &str = "Roundtable";

#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error("credential store unavailable: {0}")]
    StoreUnavailable(String),
    #[error("no credential stored for {0}")]
    NotFound(CredentialRef),
    #[error("credential store denied access: {0}")]
    Denied(String),
    #[error("secret is empty or exceeds {0} bytes")]
    InvalidSecret(usize),
}

/// Maximum secret size we will store. Real API keys are well under this.
pub const MAX_SECRET_BYTES: usize = 4096;

/// A secret in memory. Zeroized on drop; never printed.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: String) -> Result<Self, SecretsError> {
        if s.is_empty() || s.len() > MAX_SECRET_BYTES {
            return Err(SecretsError::InvalidSecret(MAX_SECRET_BYTES));
        }
        Ok(Secret(s))
    }
    /// Only the dispatch layer should call this, immediately before building the request.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

fn entry(r: CredentialRef) -> Result<keyring::Entry, SecretsError> {
    keyring::Entry::new(SERVICE, &r.to_string()).map_err(|e| SecretsError::StoreUnavailable(e.to_string()))
}

fn map_err(r: CredentialRef, e: keyring::Error) -> SecretsError {
    match e {
        keyring::Error::NoEntry => SecretsError::NotFound(r),
        keyring::Error::NoStorageAccess(inner) => SecretsError::Denied(inner.to_string()),
        other => SecretsError::StoreUnavailable(other.to_string()),
    }
}

/// Store or replace the secret for a credential reference.
pub fn save(r: CredentialRef, secret: Secret) -> Result<(), SecretsError> {
    entry(r)?.set_password(secret.expose()).map_err(|e| map_err(r, e))
}

/// Read a secret for dispatch. Never call from IPC handlers.
pub fn read(r: CredentialRef) -> Result<Secret, SecretsError> {
    let s = entry(r)?.get_password().map_err(|e| map_err(r, e))?;
    Secret::new(s)
}

/// Whether a secret exists, without returning it (KEY-02).
pub fn exists(r: CredentialRef) -> Result<bool, SecretsError> {
    match entry(r)?.get_password() {
        Ok(_) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(e) => Err(map_err(r, e)),
    }
}

/// Delete the secret. Deleting a missing secret is not an error (idempotent).
pub fn delete(r: CredentialRef) -> Result<(), SecretsError> {
    match entry(r)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(map_err(r, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_is_redacted_and_bounds_enforced() {
        let s = Secret::new("sk-test".into()).unwrap();
        assert_eq!(format!("{s:?}"), "Secret(<redacted>)");
        assert!(Secret::new(String::new()).is_err());
        assert!(Secret::new("x".repeat(MAX_SECRET_BYTES + 1)).is_err());
    }

    /// Real Windows Credential Manager round trip (M0 feasibility). Runs on the developer machine.
    #[test]
    fn credential_manager_round_trip() {
        let r = CredentialRef::new();
        assert!(!exists(r).unwrap());
        save(r, Secret::new("sk-roundtable-feasibility-0123456789".into()).unwrap()).unwrap();
        assert!(exists(r).unwrap());
        let back = read(r).unwrap();
        assert_eq!(back.expose(), "sk-roundtable-feasibility-0123456789");
        // replace
        save(r, Secret::new("sk-second".into()).unwrap()).unwrap();
        assert_eq!(read(r).unwrap().expose(), "sk-second");
        delete(r).unwrap();
        assert!(!exists(r).unwrap());
        delete(r).unwrap(); // idempotent
        assert!(matches!(read(r), Err(SecretsError::NotFound(_))));
    }
}

#[cfg(test)]
mod long_key_tests {
    use super::*;

    /// OpenRouter keys are 73 characters; make sure Credential Manager returns them byte for byte.
    #[test]
    fn credential_manager_preserves_long_keys_exactly() {
        let key = format!("sk-or-v1-{}", "0123456789abcdef".repeat(4));
        assert_eq!(key.len(), 73);
        let r = CredentialRef::new();
        save(r, Secret::new(key.clone()).unwrap()).unwrap();
        let back = read(r).unwrap();
        assert_eq!(back.expose(), key);
        assert_eq!(back.expose().len(), 73);
        delete(r).unwrap();
    }
}
