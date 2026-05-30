//! Configured backend instances — one row per (kind, named credential set).
//!
//! Each `BackendBinding` on a form with `instance_id = Some(_)` references a
//! row here. Built-in singletons (e.g. `open-relay`) don't need a row.
//!
//! Security note: `config` is a JSON blob that may contain secrets (API
//! tokens, refresh tokens). Stored plaintext in v1 — same caveat as
//! `oauth_provider_config.client_secret`. A follow-up should AEAD-encrypt
//! secret-bearing fields with an env-derived key.

use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ConnectionTrait};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "backend_instance")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Discriminator that picks a `BackendFactory` at delivery time, e.g.
    /// `"gohighlevel"`.
    #[sea_orm(indexed)]
    pub kind: String,
    /// Admin-supplied display label.
    pub name: String,
    /// Kind-specific configuration. Schema is owned by the matching factory;
    /// the CRUD service round-trips this through `BackendFactory::build` on
    /// write so the runtime is guaranteed to be able to load it.
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
