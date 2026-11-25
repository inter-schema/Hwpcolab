use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::error::AiError;
use crate::types::BoxFuture;

/// Abstraction over OS keyring / secret storage.
/// `keyring` crate is not available in the offline sandbox; `StubSecretStore`
/// provides an in-memory fallback until OS keyring integration is enabled.
pub trait SecretStore: Send + Sync {
    fn get_secret<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<String>, AiError>>;
    fn set_secret<'a>(
        &'a self,
        key: &'a str,
        value: &'a str,
    ) -> BoxFuture<'a, Result<(), AiError>>;
    fn delete_secret<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<bool, AiError>>;
}

#[derive(Debug, Default)]
pub struct StubSecretStore {
    entries: Arc<RwLock<HashMap<String, String>>>,
}

impl StubSecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for StubSecretStore {
    fn get_secret<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<String>, AiError>> {
        Box::pin(async move {
            let guard = self.entries.read().map_err(|e| {
                AiError::SecretStore(format!("SecretStore read lock poisoned: {e}"))
            })?;
            Ok(guard.get(key).cloned())
        })
    }

    fn set_secret<'a>(
        &'a self,
        key: &'a str,
        value: &'a str,
    ) -> BoxFuture<'a, Result<(), AiError>> {
        Box::pin(async move {
            let mut guard = self.entries.write().map_err(|e| {
                AiError::SecretStore(format!("SecretStore write lock poisoned: {e}"))
            })?;
            guard.insert(key.to_string(), value.to_string());
            Ok(())
        })
    }

    fn delete_secret<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<bool, AiError>> {
        Box::pin(async move {
            let mut guard = self.entries.write().map_err(|e| {
                AiError::SecretStore(format!("SecretStore write lock poisoned: {e}"))
            })?;
            Ok(guard.remove(key).is_some())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_secret_store_round_trip() {
        let store = StubSecretStore::new();
        assert_eq!(store.get_secret("api_key").await.unwrap(), None);
        store.set_secret("api_key", "sk-test-123").await.unwrap();
        assert_eq!(
            store.get_secret("api_key").await.unwrap().as_deref(),
            Some("sk-test-123")
        );
        assert!(store.delete_secret("api_key").await.unwrap());
        assert_eq!(store.get_secret("api_key").await.unwrap(), None);
    }
}
