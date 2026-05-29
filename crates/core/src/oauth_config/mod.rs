//! Admin-configured OAuth provider config — CRUD + DTOs.

pub mod service;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Public-facing config: rendered on the unauthenticated `/login` page so the
/// admin can choose to show a "Sign in with X" button without giving away
/// client secrets or endpoints.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OAuthConfigPublicDto {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// Admin-facing config — same shape minus `client_secret`. The admin UI
/// shows `[unchanged]` for the secret field and only sends a value when the
/// admin types one.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OAuthConfigDto {
    pub id: i32,
    pub display_name: String,
    pub discovery_url: Option<String>,
    pub issuer: Option<String>,
    pub client_id: String,
    pub authorize_url: String,
    pub token_url: String,
    pub userinfo_url: Option<String>,
    pub jwks_url: Option<String>,
    pub scopes: String,
    pub default_role_id: Option<i32>,
    pub email_claim: String,
    pub subject_claim: String,
    /// True if a non-empty client_secret is on record.
    pub has_client_secret: bool,
}

impl From<entity::oauth_provider_config::Model> for OAuthConfigDto {
    fn from(m: entity::oauth_provider_config::Model) -> Self {
        let has_client_secret = !m.client_secret.is_empty();
        Self {
            id: m.id,
            display_name: m.display_name,
            discovery_url: m.discovery_url,
            issuer: m.issuer,
            client_id: m.client_id,
            authorize_url: m.authorize_url,
            token_url: m.token_url,
            userinfo_url: m.userinfo_url,
            jwks_url: m.jwks_url,
            scopes: m.scopes,
            default_role_id: m.default_role_id,
            email_claim: m.email_claim,
            subject_claim: m.subject_claim,
            has_client_secret,
        }
    }
}

/// Upsert input. `client_secret: None` means "keep the existing value";
/// `Some(non_empty)` replaces. The service rejects creating a new config
/// without a client_secret.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct UpsertOAuthConfig {
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    pub authorize_url: String,
    pub token_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub userinfo_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwks_url: Option<String>,
    pub scopes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_role_id: Option<i32>,
    #[serde(default)]
    pub email_claim: Option<String>,
    #[serde(default)]
    pub subject_claim: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct DiscoveryRequest {
    pub discovery_url: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DiscoveryPrefill {
    pub issuer: String,
    pub authorize_url: String,
    pub token_url: String,
    pub userinfo_url: Option<String>,
    pub jwks_url: Option<String>,
    pub scopes_supported: Option<Vec<String>>,
}
