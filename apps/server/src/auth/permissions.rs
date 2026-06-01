//! Permission-check helper used by route handlers.
//!
//! Pattern: every handler that needs an authorization gate calls
//! `require_permission(&state, claims, Permission::Foo).await?` at the top.
//! Returns an `AuthorizedUser` bundling the parsed user id + original
//! claims so handlers don't repeat the `claims.sub.parse()` boilerplate.

use std::collections::HashSet;

use open_relay_core::auth::Claims;
use open_relay_core::permissions::Permission;
use open_relay_core::rbac::service as rbac_service;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub struct AuthorizedUser {
    pub id: i32,
    pub claims: Claims,
    /// The actor's full permission set, loaded during the check. Reused by
    /// handlers that need it (e.g. the role-assignment escalation guard) so
    /// they don't re-query.
    pub permissions: HashSet<Permission>,
}

/// Verify the holder of `claims` is granted `needed`. Returns the parsed
/// user id on success; `Unauthorized` if the subject is unparseable or
/// missing, `Forbidden` if the permission is not held.
pub async fn require_permission(
    state: &AppState,
    claims: Claims,
    needed: Permission,
) -> AppResult<AuthorizedUser> {
    let id: i32 = claims.sub.parse().map_err(|_| AppError::Unauthorized)?;
    let permissions = rbac_service::load_user_permissions(&state.db, id).await?;
    if !permissions.contains(&needed) {
        return Err(AppError::Forbidden(format!(
            "missing permission: {}",
            needed.slug()
        )));
    }
    Ok(AuthorizedUser {
        id,
        claims,
        permissions,
    })
}

/// Authenticate without checking a permission — useful when the handler
/// only needs the parsed user id (e.g. `/auth/me`, `/permissions` catalog).
pub fn authenticated_user(claims: Claims) -> AppResult<AuthorizedUser> {
    let id: i32 = claims.sub.parse().map_err(|_| AppError::Unauthorized)?;
    Ok(AuthorizedUser {
        id,
        claims,
        permissions: HashSet::new(),
    })
}
