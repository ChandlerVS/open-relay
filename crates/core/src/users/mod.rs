//! User domain logic: validation, Argon2 hashing, SeaORM persistence, and the
//! wire-contract types (DTOs) that describe what crosses the API boundary.
//!
//! Framework-agnostic — `serde` and `utoipa` are pure metadata libraries, not
//! tied to any HTTP framework.

pub mod service;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Input shape for creating a user.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct NewUser {
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
}

/// Outbound representation of a user — what callers see in API responses.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UserDto {
    pub id: i32,
    pub email: String,
    pub display_name: Option<String>,
}

impl From<entity::user::Model> for UserDto {
    fn from(m: entity::user::Model) -> Self {
        Self {
            id: m.id,
            email: m.email,
            display_name: m.display_name,
        }
    }
}

/// Partial update. `None` means "leave the field alone". For `display_name`,
/// an explicit empty string is treated as "clear".
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
pub struct UpdateUser {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct ChangePassword {
    pub password: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UserList {
    pub items: Vec<UserDto>,
    pub total: u64,
    pub limit: u32,
    pub offset: u32,
}

/// Lightweight option used to populate dropdowns elsewhere in the admin.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UserSelectOption {
    pub id: i32,
    pub label: String,
}
