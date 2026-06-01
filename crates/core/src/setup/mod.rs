//! First-time bootstrap domain types.
//!
//! The HTTP route lives in the server crate; the wire-contract types belong
//! here so a non-HTTP caller (CLI seed command, integration harness) can
//! produce/consume them too.

use serde::Serialize;
use utoipa::ToSchema;

use crate::users::UserDto;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct InitializeResponse {
    pub token: String,
    pub refresh_token: String,
    pub user: UserDto,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SetupStatus {
    pub initialized: bool,
}
