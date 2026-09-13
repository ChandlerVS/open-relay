//! Long-lived API keys — the machine credential behind the MCP surface.
//!
//! # Why a third credential
//!
//! The access JWT lives 15 minutes ([`crate::auth::ACCESS_TTL_SECONDS`]) and
//! the refresh token *rotates on every use*. Both are right for a browser
//! session and wrong for an agent: an MCP client holds a static string in a
//! config file, has nowhere to write a rotated secret back to, and would start
//! failing minutes after it was set up. So an API key is opaque, long-lived,
//! and never rotates — [`entity::api_key`] explains the storage side.
//!
//! # Scopes narrow, they never grant
//!
//! A key resolves to an [`ApiActor`] carrying a permission set, and that set is
//! always `owner's live permissions ∩ key scopes`. Two consequences worth
//! stating plainly:
//!
//! 1. A key cannot outrank the user who minted it, so issuing one needs no
//!    permission of its own — it is self-service, like `/auth/me`.
//! 2. Permissions are resolved **per request**, not frozen at issue time. Strip
//!    a user's role and every key they hold narrows with it on the next call.
//!
//! `scopes: None` means "whatever the owner has" rather than "everything",
//! which is why the intersection is written as a filter rather than a default.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::permissions::Permission;

pub mod service;

/// Prefix on every issued secret. Two jobs: it makes a leaked key greppable in
/// logs and source control, and it is how the HTTP middleware tells an API key
/// from a JWT without attempting to parse either.
pub const TOKEN_PREFIX: &str = "orl_";

/// An authenticated principal and its *effective* permissions.
///
/// Deliberately framework-free so the MCP crate can consume it; the server
/// stashes one in `http::Extensions`, hence `Clone`.
#[derive(Debug, Clone)]
pub struct ApiActor {
    pub user_id: i32,
    pub permissions: HashSet<Permission>,
}

impl ApiActor {
    pub fn has(&self, needed: Permission) -> bool {
        self.permissions.contains(&needed)
    }
}

/// A key as shown back to its owner. Never carries the secret — there is no
/// field it could go in, which is the point.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ApiKeyDto {
    pub id: i32,
    pub name: String,
    /// Leading characters of the secret, for matching a row against a config
    /// file. Not sensitive.
    pub prefix: String,
    /// `None` = inherits the owner's full permission set.
    pub scopes: Option<Vec<Permission>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct NewApiKey {
    pub name: String,
    /// Omit (or send `null`) to inherit the owner's permissions. An empty list
    /// is rejected — a key that can do nothing is a configuration mistake, not
    /// a useful safety setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<Permission>>,
    /// Omit for a key that never expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in_days: Option<u32>,
}

/// The issue response. The only place `token` is ever populated — it cannot be
/// recovered afterwards, so the admin UI has to surface it immediately.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CreatedApiKey {
    #[serde(flatten)]
    pub key: ApiKeyDto,
    /// The plaintext secret. Shown once, never stored.
    pub token: String,
}
