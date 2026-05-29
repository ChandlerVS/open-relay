//! Links a local user to an identity asserted by an external OAuth provider.
//!
//! Uniqueness:
//! - `(provider_config_id, subject)`: an IdP identity maps to at most one user.
//! - `(user_id, provider_config_id)`: a user holds at most one identity per provider.
//!
//! Cleanup on user delete is handled in application code (see
//! `open_relay_core::users::service::delete_user`).

use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ConnectionTrait};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "external_identity")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(indexed)]
    pub user_id: i32,
    #[sea_orm(indexed)]
    pub provider_config_id: i32,
    /// Subject claim from the IdP (e.g. `sub`). Unique per provider.
    pub subject: String,
    /// Email asserted at link time. Kept for audit; user's email may diverge later.
    pub email_at_link: Option<String>,
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
