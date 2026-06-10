//! Code-defined permission catalogue.
//!
//! Permissions are an enum here — the single source of truth. Roles (in the
//! database) hold a *set of slugs*; the slugs are this enum's serialized form.
//! New permissions are added by extending the enum; `ensure_superadmin` on
//! boot diffs the catalogue against the superadmin role's grants so they pick
//! up automatically.
//!
//! The wire format is `"<resource>:<action>"` (e.g. `"users:read"`).
//! `Permission::from_slug` is `Option` on purpose — an unknown slug coming
//! out of the DB (a row from a previously-deployed enum variant that has
//! since been removed) is silently dropped, not propagated as a 500.

use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use utoipa::ToSchema;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, strum::EnumIter,
)]
pub enum Permission {
    #[serde(rename = "users:read")]
    UsersRead,
    #[serde(rename = "users:write")]
    UsersWrite,
    #[serde(rename = "users:delete")]
    UsersDelete,
    #[serde(rename = "roles:read")]
    RolesRead,
    #[serde(rename = "roles:write")]
    RolesWrite,
    #[serde(rename = "roles:delete")]
    RolesDelete,
    #[serde(rename = "roles:assign")]
    RolesAssign,
    #[serde(rename = "forms:read")]
    FormsRead,
    #[serde(rename = "forms:write")]
    FormsWrite,
    #[serde(rename = "forms:delete")]
    FormsDelete,
    #[serde(rename = "submissions:read")]
    SubmissionsRead,
    #[serde(rename = "submissions:retry")]
    SubmissionsRetry,
    #[serde(rename = "submissions:delete")]
    SubmissionsDelete,
    #[serde(rename = "backends:read")]
    BackendsRead,
    #[serde(rename = "backends:write")]
    BackendsWrite,
    #[serde(rename = "backends:delete")]
    BackendsDelete,
    #[serde(rename = "auth_config:write")]
    AuthConfigWrite,
}

impl Permission {
    pub fn all() -> Vec<Self> {
        <Self as IntoEnumIterator>::iter().collect()
    }

    pub fn slug(&self) -> &'static str {
        match self {
            Self::UsersRead => "users:read",
            Self::UsersWrite => "users:write",
            Self::UsersDelete => "users:delete",
            Self::RolesRead => "roles:read",
            Self::RolesWrite => "roles:write",
            Self::RolesDelete => "roles:delete",
            Self::RolesAssign => "roles:assign",
            Self::FormsRead => "forms:read",
            Self::FormsWrite => "forms:write",
            Self::FormsDelete => "forms:delete",
            Self::SubmissionsRead => "submissions:read",
            Self::SubmissionsRetry => "submissions:retry",
            Self::SubmissionsDelete => "submissions:delete",
            Self::BackendsRead => "backends:read",
            Self::BackendsWrite => "backends:write",
            Self::BackendsDelete => "backends:delete",
            Self::AuthConfigWrite => "auth_config:write",
        }
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        match s {
            "users:read" => Some(Self::UsersRead),
            "users:write" => Some(Self::UsersWrite),
            "users:delete" => Some(Self::UsersDelete),
            "roles:read" => Some(Self::RolesRead),
            "roles:write" => Some(Self::RolesWrite),
            "roles:delete" => Some(Self::RolesDelete),
            "roles:assign" => Some(Self::RolesAssign),
            "forms:read" => Some(Self::FormsRead),
            "forms:write" => Some(Self::FormsWrite),
            "forms:delete" => Some(Self::FormsDelete),
            "submissions:read" => Some(Self::SubmissionsRead),
            "submissions:retry" => Some(Self::SubmissionsRetry),
            "submissions:delete" => Some(Self::SubmissionsDelete),
            "backends:read" => Some(Self::BackendsRead),
            "backends:write" => Some(Self::BackendsWrite),
            "backends:delete" => Some(Self::BackendsDelete),
            "auth_config:write" => Some(Self::AuthConfigWrite),
            _ => None,
        }
    }

    pub fn resource(&self) -> &'static str {
        self.slug().split_once(':').map(|(r, _)| r).unwrap_or("")
    }

    pub fn action(&self) -> &'static str {
        self.slug().split_once(':').map(|(_, a)| a).unwrap_or("")
    }

    /// Human-readable label for the action, used by the role editor UI.
    pub fn label(&self) -> &'static str {
        match (self.resource(), self.action()) {
            (_, "read") => "View",
            (_, "write") => "Create & edit",
            (_, "delete") => "Delete",
            ("submissions", "retry") => "Re-trigger delivery",
            ("roles", "assign") => "Assign to users",
            _ => self.slug(),
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PermissionInfo {
    pub key: Permission,
    pub resource: String,
    pub action: String,
    pub label: String,
}

/// Static catalogue, sorted by (resource, action) so the UI can render it
/// without re-sorting. Cheap — called once per role-editor page load and
/// cached client-side for the rest of the session.
pub fn catalog() -> Vec<PermissionInfo> {
    let mut rows: Vec<PermissionInfo> = Permission::all()
        .into_iter()
        .map(|p| PermissionInfo {
            key: p,
            resource: p.resource().to_string(),
            action: p.action().to_string(),
            label: p.label().to_string(),
        })
        .collect();
    rows.sort_by(|a, b| {
        a.resource
            .cmp(&b.resource)
            .then_with(|| a.action.cmp(&b.action))
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_variants_round_trip_through_slug() {
        let all = Permission::all();
        assert!(!all.is_empty());
        for p in &all {
            assert_eq!(Permission::from_slug(p.slug()), Some(*p));
        }
    }

    #[test]
    fn all_slugs_are_unique() {
        let slugs: HashSet<&'static str> = Permission::all().iter().map(|p| p.slug()).collect();
        assert_eq!(slugs.len(), Permission::all().len());
    }

    #[test]
    fn unknown_slug_returns_none() {
        assert!(Permission::from_slug("nope:read").is_none());
    }

    #[test]
    fn catalog_is_sorted() {
        let rows = catalog();
        let mut prev: Option<(&str, &str)> = None;
        for row in &rows {
            let cur = (row.resource.as_str(), row.action.as_str());
            if let Some(p) = prev {
                assert!(p < cur, "catalog not sorted: {:?} >= {:?}", p, cur);
            }
            prev = Some(cur);
        }
    }
}
