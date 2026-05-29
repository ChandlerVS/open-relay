//! Mapping between local users and external (OAuth/OIDC) identities.

pub mod service;

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ExternalIdentityDto {
    pub id: i32,
    pub provider_config_id: i32,
    pub provider_display_name: String,
    pub email_at_link: Option<String>,
    pub created_at: DateTime<Utc>,
}
