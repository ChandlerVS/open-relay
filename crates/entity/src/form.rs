//! Form entity — a configurable form schema embedded by an external host page.
//!
//! See `crates/entity/src/lib.rs` for the entity-first pattern this follows.
//! Schema is synced into MySQL at server boot; do not edit by hand from the DB.
//!
//! `standard_fields` and `custom_fields` are JSON columns whose typed shapes
//! (`StandardFieldsConfig`, `Vec<CustomField>`) live in
//! `open_relay_core::forms` — keeping the typed wire/domain layer out of the
//! entity crate so it stays a thin SeaORM surface.

use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ConnectionTrait};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "form")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// FK to `user.id`. The creator at the time of insert. FK cleanup runs in
    /// application code (see `open_relay_core::users::service::delete_user`).
    pub owner_id: i32,
    pub name: String,
    #[sea_orm(unique)]
    pub slug: String,
    #[sea_orm(column_type = "Json")]
    pub standard_fields: Json,
    #[sea_orm(column_type = "Json")]
    pub custom_fields: Json,
    /// Ordered list of `BackendBinding` entries (see
    /// `open_relay_core::forms`). Each entry queues one delivery row per
    /// submission. Nullable for back-compat with rows created before this
    /// column existed; the boot-time backfill in
    /// `open_relay_core::forms::service::backfill_default_backends` sets any
    /// `NULL`s to `[{ "name": "open-relay" }]`.
    #[sea_orm(column_type = "Json", nullable)]
    pub backends: Option<Json>,
    /// Tags attached to every submission from this form. Dispatched to
    /// backends via `DeliveryPayload`. Stored as a JSON array of strings.
    /// `NULL` is equivalent to an empty list (back-compat).
    #[sea_orm(column_type = "Json", nullable)]
    pub tags: Option<Json>,
    /// Sales reps this form offers, as a JSON array of `sales_rep.id` values.
    /// A submission's `?rep=<key>` is resolved against this set. `NULL` /
    /// missing is equivalent to an empty list. See `open_relay_core::forms`.
    #[sea_orm(column_type = "Json", nullable)]
    pub reps: Option<Json>,
    /// Extra URL query params to capture from the QR landing page and emit as
    /// per-submission tags. JSON array of `SourceParam` (`{ param, tag_prefix }`)
    /// — see `open_relay_core::forms`. `NULL` is equivalent to an empty list.
    #[sea_orm(column_type = "Json", nullable)]
    pub source_params: Option<Json>,
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
