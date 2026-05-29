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
