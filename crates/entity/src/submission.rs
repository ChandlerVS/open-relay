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
    /// Resolved sales rep this submission was attributed to (from the QR
    /// landing URL's `?rep=<key>`), or `NULL` if none matched. No DB FK
    /// constraint — cleanup on rep delete runs in application code
    /// (`open_relay_core::reps::service::delete_rep`).
    #[sea_orm(nullable)]
    pub sales_rep_id: Option<i32>,
    /// Raw source params captured from the QR landing URL (e.g.
    /// `{"event":"mjbiz-2026"}`), kept for the audit trail. `NULL` when none
    /// were captured.
    #[sea_orm(column_type = "Json", nullable)]
    pub source_params: Option<Json>,
    /// Set when the submission matched an existing email for the form and email
    /// deduplication (`MetadataKey::EmailDeduplication`) was enabled. Duplicates
    /// are still stored (the audit log accepts them and the submitter sees
    /// success) but get no `submission_delivery` rows — they are never
    /// dispatched to any backend. Nullable for back-compat with rows created
    /// before this column existed (and so the additive schema sync can add it
    /// to populated tables); `NULL` is equivalent to `false`.
    #[sea_orm(nullable)]
    pub is_duplicate: Option<bool>,
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
