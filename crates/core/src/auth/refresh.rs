//! Refresh-token issuance, rotation, and revocation.
//!
//! The opaque secret is returned to the client once; only its SHA-256 hash is
//! stored (`refresh_token` entity). Every `/auth/refresh` *rotates*: the
//! presented row is revoked and a new one issued, so a leaked-then-used token
//! is single-use. Presenting an already-revoked token is treated as a reuse
//! breach and revokes the user's whole session set.

use base64::Engine;
use chrono::Utc;
use rand::RngCore;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
};
use sha2::{Digest, Sha256};

use crate::auth::REFRESH_TTL_SECONDS;
use crate::error::{CoreError, CoreResult};

/// Generate a 256-bit opaque refresh secret (URL-safe base64, no padding).
pub fn generate_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// SHA-256 (hex) of a refresh secret. A 256-bit random secret needs no slow
/// KDF — it isn't guessable — so a fast digest is appropriate here.
pub fn hash_secret(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Issue a new refresh token for `user_id` and return the plaintext secret.
pub async fn issue<C: ConnectionTrait>(conn: &C, user_id: i32) -> CoreResult<String> {
    let secret = generate_secret();
    let expires_at = Utc::now() + chrono::Duration::seconds(REFRESH_TTL_SECONDS);
    let active = entity::refresh_token::ActiveModel {
        user_id: ActiveValue::Set(user_id),
        token_hash: ActiveValue::Set(hash_secret(&secret)),
        expires_at: ActiveValue::Set(expires_at),
        revoked_at: ActiveValue::Set(None),
        ..Default::default()
    };
    active.insert(conn).await?;
    Ok(secret)
}

/// Exchange a presented refresh secret for a new one, returning the owning user
/// and the rotated secret. The caller should run this inside a transaction.
///
/// Errors with [`CoreError::Unauthorized`] for any invalid/expired/revoked
/// token (callers must not distinguish, to avoid an oracle).
pub async fn rotate<C: ConnectionTrait>(
    conn: &C,
    presented: &str,
) -> CoreResult<(entity::user::Model, String)> {
    let hash = hash_secret(presented);
    let row = entity::refresh_token::Entity::find()
        .filter(entity::refresh_token::Column::TokenHash.eq(hash))
        .one(conn)
        .await?
        .ok_or(CoreError::Unauthorized)?;

    // Reuse of an already-revoked token => the secret leaked. Burn the whole
    // session set defensively and reject.
    if row.revoked_at.is_some() {
        revoke_all_for_user(conn, row.user_id).await?;
        return Err(CoreError::Unauthorized);
    }
    if row.expires_at < Utc::now() {
        return Err(CoreError::Unauthorized);
    }

    let user_id = row.user_id;
    // Revoke the presented row.
    let mut active: entity::refresh_token::ActiveModel = row.into();
    active.revoked_at = ActiveValue::Set(Some(Utc::now()));
    active.update(conn).await?;

    let user = entity::user::Entity::find_by_id(user_id)
        .one(conn)
        .await?
        .ok_or(CoreError::Unauthorized)?;

    let secret = issue(conn, user_id).await?;
    Ok((user, secret))
}

/// Revoke every active refresh token for a user (logout, password change,
/// privilege change). Idempotent.
pub async fn revoke_all_for_user<C: ConnectionTrait>(conn: &C, user_id: i32) -> CoreResult<()> {
    entity::refresh_token::Entity::update_many()
        .col_expr(
            entity::refresh_token::Column::RevokedAt,
            Expr::value(Some(Utc::now())),
        )
        .filter(entity::refresh_token::Column::UserId.eq(user_id))
        .filter(entity::refresh_token::Column::RevokedAt.is_null())
        .exec(conn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_unique_and_hashable() {
        let a = generate_secret();
        let b = generate_secret();
        assert_ne!(a, b);
        assert_eq!(hash_secret(&a), hash_secret(&a));
        assert_ne!(hash_secret(&a), hash_secret(&b));
        // SHA-256 hex is 64 chars.
        assert_eq!(hash_secret(&a).len(), 64);
    }
}
