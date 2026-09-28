//! API keys in the OS keychain (docs/PLAN.md 6.1 and 9). Keys stay in Rust:
//! never logged, never sent to the webview, never written anywhere else.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, PoisonError};

/// Where secrets are kept. The keychain in the app; memory in tests.
pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError>;
    fn delete(&self, account: &str) -> Result<(), SecretError>;
}

#[derive(Debug)]
pub struct SecretError(String);

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "keychain: {}", self.0)
    }
}

impl std::error::Error for SecretError {}

/// Secrets per account (one per engine), cached in memory after first use.
pub struct Secrets {
    store: Box<dyn SecretStore>,
    cache: Mutex<HashMap<String, String>>,
}

impl Secrets {
    pub fn new(store: Box<dyn SecretStore>) -> Self {
        Self {
            store,
            cache: Mutex::default(),
        }
    }

    pub fn get(&self, account: &str) -> Option<String> {
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(secret) = cache.get(account) {
            return Some(secret.clone());
        }
        match self.store.get(account) {
            Ok(Some(secret)) => {
                cache.insert(account.to_string(), secret.clone());
                Some(secret)
            }
            Ok(None) => None,
            Err(e) => {
                log::warn!("reading a secret failed: {e}");
                None
            }
        }
    }

    pub fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        let secret = secret.trim();
        if secret.is_empty() {
            return Err(SecretError("empty secret".into()));
        }
        self.store.set(account, secret)?;
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(account.to_string(), secret.to_string());
        Ok(())
    }

    pub fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(account);
        self.store.delete(account)
    }
}

/// The OS keychain (macOS Keychain, Windows Credential Manager).
pub struct Keychain {
    service: String,
}

impl Keychain {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, account: &str) -> Result<keyring::v1::Entry, SecretError> {
        keyring::v1::Entry::new(&self.service, account).map_err(|e| SecretError(e.to_string()))
    }
}

impl SecretStore for Keychain {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        match self.entry(account)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::v1::Error::NoEntry) => Ok(None),
            Err(e) => Err(SecretError(e.to_string())),
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        self.entry(account)?
            .set_password(secret)
            .map_err(|e| SecretError(e.to_string()))
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::v1::Error::NoEntry) => Ok(()),
            Err(e) => Err(SecretError(e.to_string())),
        }
    }
}

/// An in-memory store for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(account.into(), secret.into());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.0.lock().unwrap().remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_trimmed_cached_and_removable() {
        let secrets = Secrets::new(Box::new(MemoryStore::default()));
        assert_eq!(secrets.get("engine"), None);
        assert!(secrets.set("engine", "   ").is_err());
        secrets.set("engine", "  sk-test \n").unwrap();
        assert_eq!(secrets.get("engine").as_deref(), Some("sk-test"));
        secrets.delete("engine").unwrap();
        assert_eq!(secrets.get("engine"), None);
    }
}
