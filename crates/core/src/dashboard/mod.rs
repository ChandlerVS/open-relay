//! Dashboard domain logic: read-only aggregate overview for the admin landing
//! page.
//!
//! Nothing here mutates state — it rolls up counts across the existing
//! entities (`user`, `form`, `submission`, `submission_delivery`,
//! `backend_instance`) into a single payload so the SPA's first screen can
//! render in one request. `recent_submissions` is gated by the caller: the
//! server passes `include_recent = false` for users without `submissions:read`,
//! in which case the field serialises to `null`.

pub mod service;

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;

/// Headline entity counts shown as stat cards.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DashboardTotals {
    pub users: u64,
    pub forms: u64,
    pub submissions: u64,
    pub backends: u64,
}

/// One bucket of the delivery-status breakdown (e.g. `succeeded` → 42).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeliveryStatusCount {
    pub status: String,
    pub count: u64,
}

/// A form ranked by how many submissions it has received.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FormSubmissionCount {
    pub form_id: i32,
    pub form_name: String,
    pub count: u64,
}

/// A condensed submission row for the recent-activity feed. Deliberately
/// minimal — the full record is available via `/submissions/{id}`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RecentSubmission {
    pub id: i32,
    pub form_id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Aggregate payload backing the admin dashboard. `recent_submissions` is
/// `None` (serialised as `null`) when the caller lacks `submissions:read`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DashboardOverview {
    pub totals: DashboardTotals,
    pub delivery_status: Vec<DeliveryStatusCount>,
    pub top_forms: Vec<FormSubmissionCount>,
    pub recent_submissions: Option<Vec<RecentSubmission>>,
}
