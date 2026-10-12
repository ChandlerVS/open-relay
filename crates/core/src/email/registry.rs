//! Registry of known email-provider kinds.
//!
//! Mirrors [`crate::storage::registry::StorageRegistry`]: every sender needs
//! credentials, so every kind is factory-built from an `email_provider` row.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use utoipa::ToSchema;

use super::EmailSenderFactory;

/// Catalog entry returned by [`EmailRegistry::kinds`] — drives the admin's
/// provider picker.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq, Eq)]
pub struct EmailKindInfo {
    pub kind: String,
    /// Human label, so the admin UI doesn't keep its own kind → name mapping.
    pub label: String,
    /// Secret-bearing config keys, so the UI knows which inputs to render as
    /// write-only password fields without hardcoding them per kind.
    pub secret_keys: Vec<String>,
}

#[derive(Default, Clone)]
pub struct EmailRegistry {
    factories: HashMap<&'static str, Arc<dyn EmailSenderFactory>>,
}

impl EmailRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a factory for an email kind. Each `email_provider` row of this
    /// kind is built on demand by passing its `config` JSON to the factory.
    pub fn register_factory(&mut self, factory: Arc<dyn EmailSenderFactory>) {
        self.factories.insert(factory.kind(), factory);
    }

    pub fn get_factory(&self, kind: &str) -> Option<Arc<dyn EmailSenderFactory>> {
        self.factories.get(kind).cloned()
    }

    /// Secret-bearing `config` keys for a kind (empty for unknown kinds).
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
    pub fn kinds(&self) -> Vec<EmailKindInfo> {
        let mut out: Vec<EmailKindInfo> = self
            .factories
            .values()
            .map(|f| EmailKindInfo {
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
    use crate::email::ses::{self, SesFactory};

    fn registry() -> EmailRegistry {
        let mut r = EmailRegistry::new();
        r.register_factory(Arc::new(SesFactory::new()));
        r
    }

    #[test]
    fn knows_registered_kind_only() {
        let r = registry();
        assert!(r.knows(ses::KIND));
        assert!(!r.knows("smtp"));
    }

    #[test]
    fn secret_keys_are_empty_for_unknown_kind() {
        assert!(registry().secret_keys("smtp").is_empty());
        assert_eq!(registry().secret_keys(ses::KIND), &["secret_access_key"]);
    }

    #[test]
    fn kinds_reports_label_and_secrets() {
        let kinds = registry().kinds();
        assert_eq!(kinds.len(), 1);
        assert_eq!(kinds[0].kind, "ses");
        assert_eq!(kinds[0].secret_keys, vec!["secret_access_key".to_string()]);
    }
}
