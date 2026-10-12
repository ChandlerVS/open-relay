//! Outbound-email provider configuration, edited entirely via the admin UI.
//!
//! The same shape as [`super::storage_provider`]: one row with
//! `is_active = true` (enforced by the application), carrying a
//! kind-discriminated JSON config blob.
//!
//! Security note: secret-bearing keys inside `config` (declared per kind via
//! `EmailSenderFactory::secret_keys`, e.g. an SES `secret_access_key`) are
//! AEAD-encrypted at rest in place (`enc:v1:<base64>` via
//! `core::crypto::SecretCipher`, keyed by `ENCRYPTION_KEY`). They are decrypted
//! only just before `EmailSenderFactory::build`. The admin DTO strips them and
//! surfaces presence as `secret_fields`, never the value.

use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ConnectionTrait};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "email_provider")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Discriminator that picks an `EmailSenderFactory`. Always `"ses"` today.
    #[sea_orm(indexed)]
    pub kind: String,
    /// Admin-supplied display label.
    pub name: String,
    pub is_active: bool,
    /// Kind-specific configuration. Schema is owned by the matching factory;
    /// the CRUD service round-trips this through `EmailSenderFactory::build`
    /// on write, so the runtime is guaranteed to be able to load it.
    #[sea_orm(column_type = "Json")]
    pub config: Json,
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
