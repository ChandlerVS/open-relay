//! Submission domain logic: validation, persistence, and DTOs.
//!
//! A submission is one accepted POST to a form. The `submission` row is the
//! canonical audit log + delivery queue; per-backend delivery state lives on
//! `submission_delivery` rows (one per backend bound to the form). The worker
//! in [`crate::jobs::worker`] consumes those.

pub mod service;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Public-facing POST body. Keys are either standard field keys (matching
/// [`crate::forms::STANDARD_FIELD_KEYS`]) or custom field keys on the form.
/// Unknown keys, and standard keys for fields that are disabled on the form,
/// are dropped silently.
///
/// Values are kept as raw JSON so the same body can carry strings, booleans
/// (checkboxes), and numbers. Standard fields are always coerced to strings;
/// custom fields are stored as-typed.
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
#[serde(transparent)]
pub struct NewSubmissionPayload(pub HashMap<String, serde_json::Value>);

/// Returned to the embed SDK on a successful POST. Deliberately minimal —
/// the public caller already has the data it sent us; echoing it back wastes
/// bytes and leaks side effects (e.g. trim normalisation) the SDK doesn't
/// need to see.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SubmissionAcceptedDto {
    pub id: i32,
}

/// Admin-facing view of a delivery row.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SubmissionDeliveryDto {
    pub id: i32,
    pub backend_name: String,
    pub status: String,
    pub attempts: i32,
    pub next_attempt_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Admin-facing view of a submission. All 14 standard columns are returned
/// individually; everything else is in `custom_data`. `deliveries` is loaded
/// on detail and list endpoints — there are at most a small fixed number per
/// submission, so the N+1 is bounded by `forms × backends`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SubmissionDto {
    pub id: i32,
    pub form_id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_line_1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_line_2: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postal_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    pub custom_data: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub deliveries: Vec<SubmissionDeliveryDto>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form_id: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SubmissionList {
    pub items: Vec<SubmissionDto>,
    pub total: u64,
    pub limit: u32,
    pub offset: u32,
}
