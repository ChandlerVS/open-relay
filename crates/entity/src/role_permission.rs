//! Join table: which permission slugs are granted by which role.
//!
//! Composite PK on `(role_id, permission)`. The `permission` column stores
//! the slug form of `open_relay_core::permissions::Permission` (e.g.
//! `"users:read"`) — slugs that no longer correspond to an enum variant are
//! filtered out at read time rather than treated as errors.
//!
//! Cleanup of dependent rows when a role is deleted is handled in
//! application code (see `open_relay_core::rbac::service::delete_role`).

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "role_permission")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub role_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub permission: String,
}

impl ActiveModelBehavior for ActiveModel {}
