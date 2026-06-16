//! Sales reps — a standalone directory resource. CRUD + DTOs.
//!
//! A rep is defined once and associated with any number of forms (via the
//! form's `reps` list). QR landing URLs carry `?rep=<key>`; the submission
//! path resolves that against the reps a form offers and attributes the lead
//! (see [`crate::submissions`]). For GoHighLevel deliveries, `ghl_user_id`
//! becomes the contact owner.
//!
//! Framework-agnostic — `serde`/`utoipa` derives are pure metadata.

pub mod service;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Outbound representation of a sales rep.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RepDto {
    pub id: i32,
    /// URL-safe slug used as `?rep=<key>` on QR landing URLs.
    pub key: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// GoHighLevel user id; when set, deliveries to a GHL backend assign the
    /// contact to this owner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ghl_user_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<entity::sales_rep::Model> for RepDto {
    fn from(m: entity::sales_rep::Model) -> Self {
        Self {
            id: m.id,
            key: m.key,
            name: m.name,
            email: m.email,
            ghl_user_id: m.ghl_user_id,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

/// Input for creating a rep. `key` defaults to a slugified `name` when omitted.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct NewRep {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ghl_user_id: Option<String>,
}

/// Partial update. `None` means "leave the field alone". For `email` /
/// `ghl_user_id`, an explicit empty string clears the value.
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
pub struct UpdateRep {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ghl_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RepList {
    pub items: Vec<RepDto>,
    pub total: u64,
}
