//! User domain logic: validation, Argon2 hashing, SeaORM persistence, and the
//! wire-contract types (DTOs) that describe what crosses the API boundary.
//!
//! Framework-agnostic — `serde` and `utoipa` are pure metadata libraries, not
//! tied to any HTTP framework.

pub mod service;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::rbac::RoleSummary;

/// Input shape for creating a user. `role_ids` is optional and defaults to
/// an empty set — the route handler reads it and calls into the RBAC service
/// in the same transaction after the user row is inserted.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct NewUser {
    pub email: String,
    pub password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default)]
    pub role_ids: Vec<i32>,
}

/// Outbound representation of a user — what callers see in API responses.
///
/// `roles` is populated by `service::populate_roles` (or by the RBAC service
/// directly for single-user lookups). `From<user::Model>` produces an empty
/// roles vec; the responsibility to enrich falls to the caller so this type
/// stays cheap to construct in places (login, setup) where the frontend will
/// refetch its session shape via `/auth/me` anyway.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UserDto {
    pub id: i32,
    pub email: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub roles: Vec<RoleSummary>,
}

impl From<entity::user::Model> for UserDto {
    fn from(m: entity::user::Model) -> Self {
        Self {
            id: m.id,
            email: m.email,
            display_name: m.display_name,
            roles: Vec::new(),
        }
    }
}

/// Partial update. `None` means "leave the field alone". For `display_name`,
/// an explicit empty string is treated as "clear".
///
/// `role_ids` semantics are asymmetric vs `display_name`: `None` leaves
/// assignments alone, `Some(vec![])` clears all roles, `Some(non-empty)`
/// replaces the set. The role-assignment guard (last-superadmin) lives in
/// `crate::rbac::service::assign_roles_to_user`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
pub struct UpdateUser {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_ids: Option<Vec<i32>>,
}

/// Admin-initiated password reset for another user. No current-password proof
/// (the actor is acting by permission, not as the target), but the service
/// forbids resetting a user who outranks the actor and revokes the target's
/// sessions. See `service::admin_set_password`.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct AdminSetPassword {
    pub password: String,
}

/// Self-service password change. Requires proof of the current password and
/// revokes the user's other sessions. See `service::change_own_password`.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct ChangeOwnPassword {
    pub current_password: String,
    pub new_password: String,
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
