//! Admin-configured backend instances — CRUD + DTOs.
//!
//! Each row here is a named credential set for one configurable backend
//! kind (today: only `gohighlevel`). Static singletons like `open-relay`
//! don't get rows — they're attached to forms by kind alone.

pub mod service;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Admin-facing instance shape. The `config` JSON round-trips verbatim —
/// the client owns kind-specific schemas. Secret fields are kept in `config`
/// rather than promoted to typed columns so future kinds can ship without
/// new migrations.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BackendInstanceDto {
    pub id: i32,
    pub kind: String,
    pub name: String,
    pub config: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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

impl From<entity::backend_instance::Model> for BackendInstanceDto {
    fn from(m: entity::backend_instance::Model) -> Self {
        Self {
            id: m.id,
            kind: m.kind,
            name: m.name,
            config: m.config,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}
