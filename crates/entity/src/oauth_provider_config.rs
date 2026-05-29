//! OAuth/OIDC provider configuration, edited entirely via the admin UI.
//!
//! Schema permits multiple rows for future multi-provider support, but the
//! application enforces exactly one row with `is_active = true` at any time.
//!
//! Security note: `client_secret` is stored as plaintext in v1. A follow-up
//! should AEAD-encrypt it with an env-derived key.

use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ConnectionTrait};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "oauth_provider_config")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Discriminator for future SAML/etc. providers. Always `"oidc"` for now.
    pub kind: String,
    /// Human label rendered in the login button: "Sign in with {display_name}".
    pub display_name: String,
    pub is_active: bool,
    pub discovery_url: Option<String>,
    pub issuer: Option<String>,
    pub client_id: String,
    pub client_secret: String,
    pub authorize_url: String,
    pub token_url: String,
    pub userinfo_url: Option<String>,
    pub jwks_url: Option<String>,
    /// Space-separated scopes, e.g. `"openid email profile"`.
    pub scopes: String,
    /// Role assigned to users auto-created on first OAuth sign-in.
    pub default_role_id: Option<i32>,
    pub email_claim: String,
    pub subject_claim: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(mut self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        let now = Utc::now();
        if insert {
            self.created_at = ActiveValue::Set(now);
        }
        self.updated_at = ActiveValue::Set(now);
        Ok(self)
    }
}
