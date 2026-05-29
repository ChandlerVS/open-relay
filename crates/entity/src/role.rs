//! Role entity — a named bag of permissions, assignable to users.
//!
//! See `crates/entity/src/lib.rs` for the entity-first pattern. Permissions
//! themselves are code-defined (`open_relay_core::permissions::Permission`);
//! the rows in `role_permission` reference roles defined here and hold the
//! permission's serialized slug.

use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ConnectionTrait};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "role")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique)]
    pub name: String,
    pub description: Option<String>,
    /// True for the auto-managed `Superadmin` role. The HTTP layer rejects
    /// edits/deletes against system roles; the auth seeder updates the row's
    /// permissions on each boot.
    pub is_system: bool,
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
