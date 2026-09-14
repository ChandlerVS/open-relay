//! Theme entity — a reusable look (colours, radius, typography, density) that
//! forms opt into via `form.theme_id`.
//!
//! `settings` is a `ThemeSettings` (see `open_relay_core::themes`) stored as
//! JSON, keeping the typed wire/domain layer out of this crate. Every member of
//! that shape is optional, so a new token never needs a migration.
//!
//! At most one row has `is_default` set; forms with no `theme_id` render with
//! it. The invariant is kept by `open_relay_core::themes::service`, not by the
//! database — MySQL can't express "unique among `true`" on a boolean.

use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ConnectionTrait};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "theme")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Admin-supplied label.
    pub name: String,
    /// The workspace default: what a form with no `theme_id` renders with.
    #[sea_orm(indexed)]
    pub is_default: bool,
    #[sea_orm(column_type = "Json")]
    pub settings: Json,
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
