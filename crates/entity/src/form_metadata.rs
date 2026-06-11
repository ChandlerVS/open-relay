//! Form metadata — a dynamic Entity-Attribute-Value (EAV) store for per-form
//! options.
//!
//! See `crates/entity/src/lib.rs` for the entity-first pattern this follows.
//! Schema is synced into MySQL at server boot; do not edit by hand from the DB.
//!
//! Each row is one `(form_id, key, value)` attribute on a form. The composite
//! PK on `(form_id, key)` enforces a single value per attribute per form. `key`
//! is the slug of a `open_relay_core::metadata::MetadataKey` and `value` is the
//! string-encoded payload whose interpretation is decided by that key's
//! declared value type — keeping the typed layer out of the entity crate so it
//! stays a thin SeaORM surface (same split as `form.standard_fields`).
//!
//! FK cleanup runs in application code (see
//! `open_relay_core::metadata::service::delete_for_form`), as elsewhere in this
//! crate — there are no DB-level foreign keys.

use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ConnectionTrait};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "form_metadata")]
pub struct Model {
    /// FK to `form.id`.
    #[sea_orm(primary_key, auto_increment = false)]
    pub form_id: i32,
    /// `MetadataKey` slug (e.g. `"email_deduplication"`).
    #[sea_orm(primary_key, auto_increment = false)]
    pub key: String,
    /// String-encoded value; decoded per the key's value type.
    #[sea_orm(column_type = "Text")]
    pub value: String,
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
