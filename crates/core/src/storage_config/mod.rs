//! Admin-configured object storage — CRUD + DTOs.
//!
//! Exactly one row is active at a time, the same stance
//! [`crate::oauth_config`] takes: file storage is deployment-wide
//! infrastructure, not a per-form choice, so every `file` field on every form
//! uses whatever is configured here.
//!
//! Secrets follow the [`crate::backends`] discipline: the kind's factory
//! declares them, [`StorageConfigDto::from_model`] strips them and reports
//! presence, and the value never leaves the server.

pub mod service;

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::storage::StorageRegistry;

/// Admin-facing config. Secret-bearing keys (declared per kind via
/// [`crate::storage::FileStoreFactory::secret_keys`]) are **stripped** from
/// `config` and surfaced as presence booleans in `secret_fields`, so the live
/// credential never reaches the client, the browser cache, or the generated
/// OpenAPI client. Mirrors `BackendInstanceDto`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StorageConfigDto {
    pub id: i32,
    pub kind: String,
    pub name: String,
    /// Non-secret config keys, verbatim. Secret keys are removed.
    pub config: serde_json::Value,
    /// For each secret key this kind declares: `true` if a non-empty value is
    /// on record. Lets the admin UI render "set / not set" without leaking it.
    pub secret_fields: BTreeMap<String, bool>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl StorageConfigDto {
    /// Build a redacted DTO from a row, consulting the registry for the kind's
    /// secret keys. Deliberately not a blanket `From<Model>` — the same reason
    /// `BackendInstanceDto` isn't one: an accidental `.into()` somewhere would
    /// serialize the credential.
    pub fn from_model(registry: &StorageRegistry, m: entity::storage_provider::Model) -> Self {
        let mut config = m.config;
        let secret_fields = crate::secrets::redact(registry.secret_keys(&m.kind), &mut config);
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

/// Upsert input. A secret key omitted from `config` (or sent empty) means
/// "keep the existing value" — the admin UI can't echo back what it was never
/// given. Creating a config with no secret fails in the factory instead.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct UpsertStorageConfig {
    pub kind: String,
    pub name: String,
    pub config: serde_json::Value,
}

/// Result of the admin's "Test connection" probe.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StorageTestResult {
    pub ok: bool,
    /// Present only on failure, and phrased for an admin to act on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::S3Factory;
    use std::sync::Arc;

    fn registry() -> StorageRegistry {
        let mut r = StorageRegistry::new();
        r.register_factory(Arc::new(S3Factory::new()));
        r
    }

    fn model(config: serde_json::Value) -> entity::storage_provider::Model {
        entity::storage_provider::Model {
            id: 1,
            kind: "s3".into(),
            name: "Uploads".into(),
            is_active: true,
            config,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn from_model_strips_secret_and_flags_presence() {
        let dto = StorageConfigDto::from_model(
            &registry(),
            model(serde_json::json!({
                "bucket": "uploads",
                "access_key_id": "AKIA…",
                "secret_access_key": "super-secret",
            })),
        );
        assert!(dto.config.get("secret_access_key").is_none());
        assert_eq!(dto.config.get("bucket").unwrap(), "uploads");
        // The access key id is an identifier, not a credential — it stays.
        assert_eq!(dto.config.get("access_key_id").unwrap(), "AKIA…");
        assert_eq!(dto.secret_fields.get("secret_access_key"), Some(&true));
    }

    #[test]
    fn from_model_marks_absent_secret_as_not_set() {
        let dto = StorageConfigDto::from_model(&registry(), model(serde_json::json!({})));
        assert_eq!(dto.secret_fields.get("secret_access_key"), Some(&false));
    }

    #[test]
    fn serialized_dto_never_contains_the_secret() {
        let dto = StorageConfigDto::from_model(
            &registry(),
            model(serde_json::json!({ "secret_access_key": "super-secret" })),
        );
        let json = serde_json::to_string(&dto).unwrap();
        assert!(!json.contains("super-secret"), "{json}");
    }
}
