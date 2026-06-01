//! Server-side refresh tokens backing the access+refresh session model.
//!
//! The access JWT is short-lived and stateless; long-lived sessions are
//! carried by an opaque refresh secret whose SHA-256 hash is stored here. This
//! is what makes sessions *revocable*: `revoked_at` is stamped on logout,
//! password change, or token-reuse detection, and a row past `expires_at` is
//! dead. The plaintext secret is never persisted.
//!
//! Rotation: each successful `/auth/refresh` revokes the presented row and
//! inserts a fresh one (see `open_relay_core::auth::refresh`).

use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ConnectionTrait};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "refresh_token")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(indexed)]
    pub user_id: i32,
    /// SHA-256 (hex) of the opaque refresh secret. Looked up on refresh; the
    /// plaintext is only ever held by the client.
    #[sea_orm(unique)]
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    /// `None` while active; set when the token is rotated out or revoked.
    pub revoked_at: Option<DateTime<Utc>>,
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(mut self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        if insert {
            self.created_at = ActiveValue::Set(Utc::now());
        }
        Ok(self)
    }
}
