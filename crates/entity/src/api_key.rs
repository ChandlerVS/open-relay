//! Long-lived API keys, the machine credential behind the MCP surface.
//!
//! The access JWT is deliberately short-lived (15 minutes) and the refresh
//! token rotates on every use, so neither can be pasted into an agent's config
//! file and left there. This entity is the third credential shape: opaque,
//! long-lived, revocable, and presented verbatim on every request.
//!
//! Storage mirrors [`super::refresh_token`] — only the SHA-256 hash of the
//! secret is persisted, so a database read cannot recover a working key. The
//! plaintext exists exactly once, in the response to the issuing call.
//!
//! Unlike a refresh token, an API key does **not** rotate: rotation would mean
//! writing a new secret back into the client's config on every call, which no
//! MCP client does. Revocation (`revoked_at`) and expiry (`expires_at`) are the
//! containment mechanisms instead.

use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ConnectionTrait};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "api_key")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(indexed)]
    pub user_id: i32,
    /// Owner-supplied label, e.g. "Claude Desktop". Only ever shown back to
    /// the owner — the key itself is unrecoverable, so this is the sole way to
    /// tell two keys apart when deciding which to revoke.
    pub name: String,
    /// SHA-256 (hex) of the opaque secret. The lookup key on every request.
    #[sea_orm(unique)]
    pub token_hash: String,
    /// Leading characters of the plaintext (`orl_` plus a few more), stored so
    /// the owner can match a row against the key in their config. Not secret
    /// and far too short to brute-force the rest from.
    pub prefix: String,
    /// `NULL` means the key carries whatever its owner currently has. A JSON
    /// array of permission slugs narrows it: the set is **intersected** with
    /// the owner's live permissions at auth time, so a key can never grant
    /// more than its owner holds and loses access the moment their role does.
    #[sea_orm(column_type = "Json", nullable)]
    pub scopes: Option<Json>,
    /// `None` never expires — the common case for an agent that should keep
    /// working until explicitly revoked.
    pub expires_at: Option<DateTime<Utc>>,
    /// Stamped on every successful authentication, so an owner can tell a live
    /// key from a forgotten one before revoking it.
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// `None` while active; set on revoke. Rows are kept rather than deleted so
    /// `last_used_at` survives as an audit trail.
    pub revoked_at: Option<DateTime<Utc>>,
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(mut self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        if insert {
            self.created_at = ActiveValue::Set(Utc::now());
        }
        Ok(self)
    }
}
