//! Sales rep entity — a named person a submission can be attributed to.
//!
//! A standalone directory resource (like `backend_instance`): defined once,
//! associated with any number of forms via the form's `reps` JSON column. A QR
//! code lands a visitor on a host page with `?rep=<key>`; the submission path
//! resolves that `key` against the reps a form offers and stamps
//! `submission.sales_rep_id`. The delivery worker then tags the lead with the
//! rep and, for GoHighLevel, sets the contact owner from `ghl_user_id`.
//!
//! See `crates/entity/src/lib.rs` for the entity-first pattern this follows.
//! FK cleanup (nulling `submission.sales_rep_id`) runs in application code
//! (`open_relay_core::reps::service::delete_rep`).

use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ConnectionTrait};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "sales_rep")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// URL-safe slug used as the `?rep=<key>` value on QR landing URLs. Unique.
    #[sea_orm(unique)]
    pub key: String,
    /// Admin-supplied display name (e.g. "Jane Doe").
    pub name: String,
    /// Optional contact email for the rep (informational).
    pub email: Option<String>,
    /// Optional GoHighLevel user id. When present, deliveries to a GHL backend
    /// set the contact owner (`assignedTo`) to this id.
    pub ghl_user_id: Option<String>,
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
