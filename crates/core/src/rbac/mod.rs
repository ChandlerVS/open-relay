//! Role-based access control: roles (DB rows) + assignment service.
//!
//! Permissions themselves live in `crate::permissions` as a code-defined
//! enum. Roles bundle permission slugs and are assignable to users. See
//! `service::ensure_superadmin` for the auto-managed `Superadmin` role.
//!
//! Framework-agnostic. The HTTP layer (extractors, route handlers) lives
//! in the server crate and calls into `service::*` for both queries and
//! authorization checks (`load_user_permissions`).

pub mod service;

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::permissions::Permission;

/// Identity + privilege context of whoever is performing a role assignment.
/// Used by [`service::assign_roles_to_user`] to bound escalation: a
/// non-superadmin may not grant the superadmin/system role, nor any permission
/// they don't themselves hold.
#[derive(Debug, Clone)]
pub struct AssignActor {
    pub is_superadmin: bool,
    pub permissions: HashSet<Permission>,
}

impl AssignActor {
    /// A trusted, non-user-initiated caller (bootstrap, OAuth default-role
    /// assignment). Bypasses privilege-bounding — these paths are not
    /// attacker-controllable escalation vectors.
    pub fn system() -> Self {
        Self {
            is_superadmin: true,
            permissions: HashSet::new(),
        }
    }
}

/// Full role detail — name, description, grants. Returned by `GET /roles/{id}`
/// and used by the role editor in the admin SPA.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RoleDto {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub is_system: bool,
    pub permissions: Vec<Permission>,
}

/// Lightweight role reference — enough to render a badge, populate a
/// dropdown, or describe the current user's roles in `/auth/me`. Cheap to
/// embed in `UserDto` since no permission join is required.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RoleSummary {
    pub id: i32,
    pub name: String,
    pub is_system: bool,
}

impl From<entity::role::Model> for RoleSummary {
    fn from(m: entity::role::Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            is_system: m.is_system,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct NewRole {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub permissions: Vec<Permission>,
}

/// Partial update. `None` means "leave alone". `description: Some("")`
/// clears (matches the convention used by `users::UpdateUser`).
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
pub struct UpdateRole {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `None` = leave assignments alone. `Some(vec)` = replace the role's
    /// permission set with this list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Vec<Permission>>,
}
