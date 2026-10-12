//! Admin-configured outbound email — CRUD + DTOs.
//!
//! Exactly one row is active at a time, the same stance
//! [`crate::storage_config`] takes: the email provider is deployment-wide
//! infrastructure, and each form only chooses a *recipient* (the
//! `notification_email` metadata key), never a provider.
//!
//! Secrets follow the [`crate::backends`] discipline: the kind's factory
//! declares them, [`EmailConfigDto::from_model`] strips them and reports
//! presence, and the value never leaves the server.

pub mod service;

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::email::EmailRegistry;

/// Admin-facing config. Secret-bearing keys (declared per kind via
/// [`crate::email::EmailSenderFactory::secret_keys`]) are **stripped** from
/// `config` and surfaced as presence booleans in `secret_fields`. Mirrors
/// `StorageConfigDto`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EmailConfigDto {
    pub id: i32,
    pub kind: String,
    pub name: String,
    /// Non-secret config keys, verbatim. Secret keys are removed.
    pub config: serde_json::Value,
    /// For each secret key this kind declares: `true` if a non-empty value is
    /// on record.
    pub secret_fields: BTreeMap<String, bool>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl EmailConfigDto {
    /// Build a redacted DTO from a row. Deliberately not a blanket
    /// `From<Model>`, so an accidental `.into()` can't serialize the secret.
    pub fn from_model(registry: &EmailRegistry, m: entity::email_provider::Model) -> Self {
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
/// "keep the existing value". Creating a config with no secret fails in the
/// factory instead.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct UpsertEmailConfig {
    pub kind: String,
    pub name: String,
    pub config: serde_json::Value,
}

/// Body of the admin's "Send test email" action.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct EmailTestRequest {
    /// Where to send the test message.
    pub to: String,
}

/// Result of the admin's "Send test email" action.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EmailTestResult {
    pub ok: bool,
    /// Present only on failure, and phrased for an admin to act on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email::SesFactory;
    use std::sync::Arc;

    fn registry() -> EmailRegistry {
        let mut r = EmailRegistry::new();
        r.register_factory(Arc::new(SesFactory::new()));
        r
    }

    fn model(config: serde_json::Value) -> entity::email_provider::Model {
        entity::email_provider::Model {
            id: 1,
            kind: "ses".into(),
            name: "Notifications".into(),
            is_active: true,
            config,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn from_model_strips_secret_and_flags_presence() {
        let dto = EmailConfigDto::from_model(
            &registry(),
            model(serde_json::json!({
                "region": "us-east-1",
                "access_key_id": "AKIA…",
                "secret_access_key": "super-secret",
                "from_address": "forms@example.com",
            })),
        );
        assert!(dto.config.get("secret_access_key").is_none());
        assert_eq!(dto.config.get("from_address").unwrap(), "forms@example.com");
        assert_eq!(dto.secret_fields.get("secret_access_key"), Some(&true));
        let json = serde_json::to_string(&dto).unwrap();
        assert!(!json.contains("super-secret"), "{json}");
    }

    #[test]
    fn from_model_marks_absent_secret_as_not_set() {
        let dto = EmailConfigDto::from_model(&registry(), model(serde_json::json!({})));
        assert_eq!(dto.secret_fields.get("secret_access_key"), Some(&false));
    }
}
