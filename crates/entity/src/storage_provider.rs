//! Object-storage provider configuration, edited entirely via the admin UI.
//!
//! Combines the two shapes already in this crate: the single-active-row rule of
//! [`super::oauth_provider_config`] (the application enforces exactly one row
//! with `is_active = true`) with the kind-discriminated JSON config blob of
//! [`super::backend_instance`].
//!
//! Security note: secret-bearing keys inside `config` (declared per kind via
//! `FileStoreFactory::secret_keys`, e.g. an S3 `secret_access_key`) are
//! AEAD-encrypted at rest in place (`enc:v1:<base64>` via
//! `core::crypto::SecretCipher`, keyed by `ENCRYPTION_KEY`). They are decrypted
//! only just before `FileStoreFactory::build`. The admin DTO strips them and
//! surfaces presence as `secret_fields`, never the value.

use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ConnectionTrait};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "storage_provider")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Discriminator that picks a `FileStoreFactory`. Always `"s3"` today.
    #[sea_orm(indexed)]
    pub kind: String,
    /// Admin-supplied display label.
    pub name: String,
    pub is_active: bool,
    /// Kind-specific configuration. Schema is owned by the matching factory;
    /// the CRUD service round-trips this through `FileStoreFactory::build` on
    /// write, so the runtime is guaranteed to be able to load it.
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
