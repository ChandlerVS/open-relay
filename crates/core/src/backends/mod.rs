//! Admin-configured backend instances — CRUD + DTOs.
//!
//! Each row here is a named credential set for one configurable backend
//! kind (today: only `gohighlevel`). Static singletons like `open-relay`
//! don't get rows — they're attached to forms by kind alone.

pub mod service;

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::backend::BackendRegistry;

/// Admin-facing instance shape. Secret-bearing keys (declared per kind via
/// [`crate::backend::BackendFactory::secret_keys`]) are **stripped** from
/// `config` and surfaced as presence booleans in `secret_fields`, so the live
/// token never reaches the client / browser cache / OpenAPI client. Mirrors
/// `OAuthConfigDto`'s `has_client_secret` redaction.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BackendInstanceDto {
    pub id: i32,
    pub kind: String,
    pub name: String,
    /// Non-secret config keys, verbatim. Secret keys are removed.
    pub config: serde_json::Value,
    /// For each secret key this kind declares: `true` if a non-empty value is
    /// on record. Lets the admin UI render "set / unset" without leaking it.
    pub secret_fields: BTreeMap<String, bool>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl BackendInstanceDto {
    /// Build a redacted DTO from a row, consulting the registry for the kind's
    /// secret keys. Use this instead of a blanket `From<Model>` so secrets are
    /// never serialized by accident.
    pub fn from_model(registry: &BackendRegistry, m: entity::backend_instance::Model) -> Self {
        let secret_keys = registry.secret_keys(&m.kind);
        let mut config = m.config;
        let mut secret_fields = BTreeMap::new();
        for key in secret_keys {
            let present = config
                .get(*key)
                .map(|v| !value_is_empty(v))
                .unwrap_or(false);
            secret_fields.insert((*key).to_string(), present);
            if let Some(obj) = config.as_object_mut() {
                obj.remove(*key);
            }
        }
        Self {
            id: m.id,
            kind: m.kind,
            name: m.name,
            config,
            secret_fields,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

/// A JSON value counts as "empty" (no secret on record) when it's null or an
/// empty/whitespace-only string.
pub(crate) fn value_is_empty(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => true,
        serde_json::Value::String(s) => s.trim().is_empty(),
        _ => false,
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct NewBackendInstance {
    pub kind: String,
    pub name: String,
    pub config: serde_json::Value,
}

/// Partial update. `None` means "leave the field alone".
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
pub struct UpdateBackendInstance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BackendInstanceList {
    pub items: Vec<BackendInstanceDto>,
    pub total: u64,
}

/// Lightweight reference used by 409 responses to tell the admin which
/// forms are blocking a delete.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BackendInstanceFormRef {
    pub id: i32,
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BackendInstanceInUse {
    pub forms: Vec<BackendInstanceFormRef>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{BackendRegistry, GoHighLevelFactory};
    use std::sync::Arc;

    fn registry() -> BackendRegistry {
        let mut r = BackendRegistry::new();
        r.register_factory(Arc::new(GoHighLevelFactory::new()));
        r
    }

    fn model(config: serde_json::Value) -> entity::backend_instance::Model {
        entity::backend_instance::Model {
            id: 1,
            kind: "gohighlevel".into(),
            name: "Acme".into(),
            config,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn from_model_strips_secret_and_flags_presence() {
        let dto = BackendInstanceDto::from_model(
            &registry(),
            model(serde_json::json!({
                "location_id": "loc_123",
                "private_integration_token": "pit-live-abc",
            })),
        );
        // Secret removed from config…
        assert!(dto.config.get("private_integration_token").is_none());
        // …non-secret field retained…
        assert_eq!(dto.config.get("location_id").unwrap(), "loc_123");
        // …and presence surfaced.
        assert_eq!(dto.secret_fields.get("private_integration_token"), Some(&true));
    }

    #[test]
    fn from_model_marks_absent_secret_as_not_present() {
        let dto = BackendInstanceDto::from_model(
            &registry(),
            model(serde_json::json!({ "location_id": "loc_123" })),
        );
        assert_eq!(dto.secret_fields.get("private_integration_token"), Some(&false));
    }
}

