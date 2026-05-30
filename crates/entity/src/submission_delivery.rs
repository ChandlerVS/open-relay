//! Submission delivery entity — one row per (submission, backend) pair.
//!
//! Status state machine, driven by `open_relay_core::jobs::worker`:
//!
//! ```text
//! pending ─lease──▶ in_progress ─success─▶ succeeded
//!    ▲                   │
//!    │                   ├── Transient ──▶ pending (next_attempt_at = now + backoff)
//!    │                   │                 OR exhausted (attempts >= MAX_ATTEMPTS)
//!    │                   ├── Permanent ─▶ permanent_failure
//!    │
//!    └── stale-lease sweep at worker startup re-queues anything stuck
//!        in `in_progress` longer than the lease deadline.
//! ```
//!
//! Cascade cleanup on submission delete is in application code.

use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ConnectionTrait};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "submission_delivery")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(indexed)]
    pub submission_id: i32,
    pub backend_name: String,
    /// One of: `pending`, `in_progress`, `succeeded`, `permanent_failure`, `exhausted`.
    pub status: String,
    pub attempts: i32,
    #[sea_orm(indexed)]
    pub next_attempt_at: DateTime<Utc>,
    #[sea_orm(column_type = "Text", nullable)]
    pub last_error: Option<String>,
    pub delivered_at: Option<DateTime<Utc>>,
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
