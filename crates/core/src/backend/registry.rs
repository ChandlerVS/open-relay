use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use utoipa::ToSchema;

use super::{Backend, BackendFactory};

/// Catalog entry returned by [`BackendRegistry::kinds`] — drives the
/// admin's "add backend" picker.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq, Eq)]
pub struct BackendKindInfo {
    pub kind: String,
    /// `true` when this kind is backed by a `BackendFactory` and so each
    /// instance needs its own row in `backend_instance`. `false` for
    /// static singletons (e.g. `open-relay`) that the admin simply attaches
    /// to a form without filling in any credentials.
    pub configurable: bool,
}

#[derive(Default, Clone)]
pub struct BackendRegistry {
    statics: HashMap<&'static str, Arc<dyn Backend>>,
    factories: HashMap<&'static str, Arc<dyn BackendFactory>>,
}

impl BackendRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a singleton backend (no per-instance config).
    pub fn register_static(&mut self, backend: Arc<dyn Backend>) {
        self.statics.insert(backend.name(), backend);
    }

    /// Register a factory for a configurable backend kind. Each
    /// `backend_instance` row of this kind will be built on demand by
    /// passing its `config` JSON to the factory.
    pub fn register_factory(&mut self, factory: Arc<dyn BackendFactory>) {
        self.factories.insert(factory.kind(), factory);
    }

    pub fn get_static(&self, kind: &str) -> Option<Arc<dyn Backend>> {
        self.statics.get(kind).cloned()
    }

    pub fn get_factory(&self, kind: &str) -> Option<Arc<dyn BackendFactory>> {
        self.factories.get(kind).cloned()
    }

    /// `true` when this registry knows about a kind (static or factory).
    pub fn knows(&self, kind: &str) -> bool {
        self.statics.contains_key(kind) || self.factories.contains_key(kind)
    }

    pub fn is_configurable(&self, kind: &str) -> bool {
        self.factories.contains_key(kind)
    }

    /// Catalogue every known kind. Sorted by kind so the admin UI gets a
    /// stable order.
    pub fn kinds(&self) -> Vec<BackendKindInfo> {
        let mut out: Vec<BackendKindInfo> = self
            .statics
            .keys()
            .map(|k| BackendKindInfo {
                kind: (*k).to_string(),
                configurable: false,
            })
            .chain(self.factories.keys().map(|k| BackendKindInfo {
                kind: (*k).to_string(),
                configurable: true,
            }))
            .collect();
        out.sort_by(|a, b| a.kind.cmp(&b.kind));
        out
    }
}
