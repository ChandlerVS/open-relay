//! Join table: which roles a user holds.
//!
//! Composite PK on `(user_id, role_id)`. Cleanup of dependent rows when
//! either side is deleted is handled in application code (see
//! `open_relay_core::users::service::delete_user` and
//! `open_relay_core::rbac::service::delete_role`).

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "user_role")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub role_id: i32,
}

impl ActiveModelBehavior for ActiveModel {}
