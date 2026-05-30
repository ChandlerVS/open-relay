//! Submission entity — one row per accepted form POST.
//!
//! Acts as the canonical audit log + delivery queue. Standard fields are
//! denormalised into typed nullable columns so the admin UI can filter/search
//! without parsing JSON; everything else lives in `custom_data`.
//!
//! Submissions are immutable once accepted (no `updated_at`). Cascade cleanup
//! on form delete is in application code (see
//! `open_relay_core::submissions::service::delete_for_form`).

use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ConnectionTrait};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "submission")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(indexed)]
    pub form_id: i32,
    // Standard fields, mirroring STANDARD_FIELD_KEYS in
    // `open_relay_core::forms`. All nullable because each form chooses which
    // standard fields to enable.
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub job_title: Option<String>,
    pub website: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub message: Option<String>,
    pub address_line_1: Option<String>,
    pub address_line_2: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    /// Custom field values, keyed by `CustomField.key`.
    #[sea_orm(column_type = "Json")]
    pub custom_data: Json,
    pub created_at: DateTime<Utc>,
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
