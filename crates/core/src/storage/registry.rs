//! Registry of known object-storage kinds.
//!
//! Mirrors [`crate::backend::registry::BackendRegistry`], minus the
//! static-singleton half: every store needs credentials, so every kind is
//! factory-built from a `storage_provider` row.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use utoipa::ToSchema;

use super::FileStoreFactory;

/// Catalog entry returned by [`StorageRegistry::kinds`] — drives the admin's
/// provider picker.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq, Eq)]
pub struct StorageKindInfo {
    pub kind: String,
    /// Human label, so the admin UI doesn't hand-maintain a second copy of the
    /// kind → display-name mapping the way the backends dialog does.
    pub label: String,
    /// Secret-bearing config keys, so the UI knows which inputs to render as
    /// write-only password fields without hardcoding them per kind.
    pub secret_keys: Vec<String>,
}

#[derive(Default, Clone)]
pub struct StorageRegistry {
    factories: HashMap<&'static str, Arc<dyn FileStoreFactory>>,
}

impl StorageRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a factory for a storage kind. Each `storage_provider` row of
    /// this kind is built on demand by passing its `config` JSON to the factory.
    pub fn register_factory(&mut self, factory: Arc<dyn FileStoreFactory>) {
        self.factories.insert(factory.kind(), factory);
    }

    pub fn get_factory(&self, kind: &str) -> Option<Arc<dyn FileStoreFactory>> {
        self.factories.get(kind).cloned()
    }

    /// Secret-bearing `config` keys for a kind (empty for unknown kinds). Used
    /// to redact admin-facing DTOs and preserve secrets across partial updates.
    pub fn secret_keys(&self, kind: &str) -> &'static [&'static str] {
        self.factories
            .get(kind)
            .map(|f| f.secret_keys())
            .unwrap_or(&[])
    }

    pub fn knows(&self, kind: &str) -> bool {
        self.factories.contains_key(kind)
    }

    /// Catalogue every known kind, sorted so the admin UI gets a stable order.
    pub fn kinds(&self) -> Vec<StorageKindInfo> {
        let mut out: Vec<StorageKindInfo> = self
            .factories
            .values()
            .map(|f| StorageKindInfo {
                kind: f.kind().to_string(),
                label: f.label().to_string(),
                secret_keys: f.secret_keys().iter().map(|k| (*k).to_string()).collect(),
            })
            .collect();
        out.sort_by(|a, b| a.kind.cmp(&b.kind));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::s3::{self, S3Factory};

    fn registry() -> StorageRegistry {
        let mut r = StorageRegistry::new();
        r.register_factory(Arc::new(S3Factory::new()));
        r
    }

    #[test]
    fn knows_registered_kind_only() {
        let r = registry();
        assert!(r.knows(s3::KIND));
        assert!(!r.knows("gcs"));
    }

    #[test]
    fn secret_keys_are_empty_for_unknown_kind() {
        assert!(registry().secret_keys("gcs").is_empty());
        assert_eq!(registry().secret_keys(s3::KIND), &["secret_access_key"]);
    }

    #[test]
    fn kinds_reports_label_and_secrets() {
        let kinds = registry().kinds();
        assert_eq!(kinds.len(), 1);
        assert_eq!(kinds[0].kind, "s3");
        assert_eq!(kinds[0].secret_keys, vec!["secret_access_key".to_string()]);
    }
}
