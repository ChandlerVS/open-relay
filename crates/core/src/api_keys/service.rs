//! Issue, authenticate, list and revoke API keys.
//!
//! The secret is generated and hashed by [`crate::auth::refresh`] — the same
//! 256-bit random value and the same fast SHA-256 digest, for the same reason
//! documented there: a 256-bit random secret is not guessable, so a slow KDF
//! buys nothing. Only the hash is persisted.

use std::collections::HashSet;

use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder,
};

use crate::api_keys::{ApiActor, ApiKeyDto, CreatedApiKey, NewApiKey, TOKEN_PREFIX};
use crate::auth::refresh::{generate_secret, hash_secret};
use crate::error::{CoreError, CoreResult};
use crate::permissions::Permission;
use crate::rbac::service as rbac_service;

const MAX_NAME_LEN: usize = 100;
/// Length of the stored display prefix, including `TOKEN_PREFIX`. Eight
/// characters is enough to tell two keys apart and far too few to attack the
/// remaining ~250 bits with.
const PREFIX_LEN: usize = 8;
/// Ten years. Not a security boundary — `revoked_at` is — just a guard against
/// an `expires_in_days` large enough to overflow the timestamp arithmetic.
const MAX_EXPIRY_DAYS: u32 = 3650;

fn validate_name(name: &str) -> CoreResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CoreError::BadRequest("api key name is required".into()));
    }
    if trimmed.chars().count() > MAX_NAME_LEN {
        return Err(CoreError::BadRequest(format!(
            "api key name must be at most {MAX_NAME_LEN} characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// Issue a key for `user_id`, returning the DTO **and** the plaintext secret.
///
/// The plaintext is a separate tuple member rather than a field on the DTO so
/// that surfacing it has to be a deliberate act at the call site.
pub async fn issue<C: ConnectionTrait>(
    conn: &C,
    user_id: i32,
    input: NewApiKey,
) -> CoreResult<CreatedApiKey> {
    let name = validate_name(&input.name)?;

    // An explicit empty scope list is a mistake, not a lockdown: it would mint
    // a key that authenticates and then fails every tool call.
    let scopes = match input.scopes {
        Some(s) if s.is_empty() => {
            return Err(CoreError::BadRequest(
                "scopes must name at least one permission, or be omitted to inherit yours".into(),
            ));
        }
        other => other,
    };

    // Scopes narrow rather than grant, so a scope the owner does not hold is
    // inert. Rejecting it anyway turns a silently useless key into an
    // actionable error at the moment the admin can still fix it.
    if let Some(requested) = &scopes {
        let held = rbac_service::load_user_permissions(conn, user_id).await?;
        if let Some(missing) = requested.iter().find(|p| !held.contains(p)) {
            return Err(CoreError::Forbidden(format!(
                "cannot scope a key to a permission you do not hold: {}",
                missing.slug()
            )));
        }
    }

    let expires_at = match input.expires_in_days {
        Some(0) => {
            return Err(CoreError::BadRequest(
                "expires_in_days must be at least 1, or omitted for a key that never expires"
                    .into(),
            ));
        }
        Some(days) if days > MAX_EXPIRY_DAYS => {
            return Err(CoreError::BadRequest(format!(
                "expires_in_days must be at most {MAX_EXPIRY_DAYS}"
            )));
        }
        Some(days) => Some(Utc::now() + chrono::Duration::days(i64::from(days))),
        None => None,
    };

    let secret = format!("{TOKEN_PREFIX}{}", generate_secret());
    let prefix: String = secret.chars().take(PREFIX_LEN).collect();

    let scopes_json = scopes
        .as_ref()
        .map(|s| serde_json::to_value(s).map_err(|e| CoreError::Internal(e.into())))
        .transpose()?;

    let active = entity::api_key::ActiveModel {
        user_id: ActiveValue::Set(user_id),
        name: ActiveValue::Set(name),
        token_hash: ActiveValue::Set(hash_secret(&secret)),
        prefix: ActiveValue::Set(prefix),
        scopes: ActiveValue::Set(scopes_json),
        expires_at: ActiveValue::Set(expires_at),
        last_used_at: ActiveValue::Set(None),
        revoked_at: ActiveValue::Set(None),
        ..Default::default()
    };
    let model = active.insert(conn).await?;

    Ok(CreatedApiKey {
        key: to_dto(model)?,
        token: secret,
    })
}

/// Resolve a presented secret to its effective actor.
///
/// Every failure — unknown, revoked, expired, or owned by a deleted user — is
/// [`CoreError::Unauthorized`], so a caller cannot use the error to probe which
/// keys exist. Same stance as [`crate::auth::refresh::rotate`].
pub async fn authenticate<C: ConnectionTrait>(conn: &C, presented: &str) -> CoreResult<ApiActor> {
    // Cheap structural reject before touching the database. Also means a
    // mistyped JWT never costs a query.
    if !presented.starts_with(TOKEN_PREFIX) {
        return Err(CoreError::Unauthorized);
    }

    let row = entity::api_key::Entity::find()
        .filter(entity::api_key::Column::TokenHash.eq(hash_secret(presented)))
        .one(conn)
        .await?
        .ok_or(CoreError::Unauthorized)?;

    if row.revoked_at.is_some() {
        return Err(CoreError::Unauthorized);
    }
    if row.expires_at.is_some_and(|exp| exp < Utc::now()) {
        return Err(CoreError::Unauthorized);
    }

    let user_id = row.user_id;

    // Permissions are read live, never frozen at issue time: revoking a role
    // has to narrow every key that user holds, immediately.
    let mut permissions = rbac_service::load_user_permissions(conn, user_id).await?;
    if let Some(scopes) = parse_scopes(&row)? {
        let allowed: HashSet<Permission> = scopes.into_iter().collect();
        permissions.retain(|p| allowed.contains(p));
    }

    // Best-effort liveness stamp. Deliberately not fatal: losing a whole
    // request because an audit column could not be written would be a worse
    // outcome than a stale `last_used_at`.
    if let Err(err) = entity::api_key::Entity::update_many()
        .col_expr(
            entity::api_key::Column::LastUsedAt,
            Expr::value(Some(Utc::now())),
        )
        .filter(entity::api_key::Column::Id.eq(row.id))
        .exec(conn)
        .await
    {
        tracing::warn!(?err, key_id = row.id, "failed to stamp api key last_used_at");
    }

    Ok(ApiActor {
        user_id,
        permissions,
    })
}

pub async fn list_for_user<C: ConnectionTrait>(
    conn: &C,
    user_id: i32,
) -> CoreResult<Vec<ApiKeyDto>> {
    let rows = entity::api_key::Entity::find()
        .filter(entity::api_key::Column::UserId.eq(user_id))
        .order_by_desc(entity::api_key::Column::Id)
        .all(conn)
        .await?;
    rows.into_iter().map(to_dto).collect()
}

/// Revoke one of `user_id`'s own keys.
///
/// The `user_id` filter is part of the lookup rather than a check afterwards,
/// so another user's key id is indistinguishable from a nonexistent one.
pub async fn revoke<C: ConnectionTrait>(conn: &C, user_id: i32, id: i32) -> CoreResult<()> {
    let res = entity::api_key::Entity::update_many()
        .col_expr(
            entity::api_key::Column::RevokedAt,
            Expr::value(Some(Utc::now())),
        )
        .filter(entity::api_key::Column::Id.eq(id))
        .filter(entity::api_key::Column::UserId.eq(user_id))
        .filter(entity::api_key::Column::RevokedAt.is_null())
        .exec(conn)
        .await?;
    if res.rows_affected == 0 {
        return Err(CoreError::NotFound("api key not found".into()));
    }
    Ok(())
}

/// Cascade hook for user deletion, mirroring `forms::service::delete_for_owner`.
///
/// Hard-deletes rather than revoking: the owner is gone, so there is no audit
/// trail left to preserve, and a dangling `user_id` would break `authenticate`.
pub async fn delete_for_owner<C: ConnectionTrait>(conn: &C, user_id: i32) -> CoreResult<()> {
    entity::api_key::Entity::delete_many()
        .filter(entity::api_key::Column::UserId.eq(user_id))
        .exec(conn)
        .await?;
    Ok(())
}

/// Decode the stored scope list. An unknown slug is dropped rather than
/// erroring, matching `load_user_permissions` — a permission removed from the
/// enum since the key was issued must not 500 every request the key makes.
fn parse_scopes(row: &entity::api_key::Model) -> CoreResult<Option<Vec<Permission>>> {
    let Some(value) = &row.scopes else {
        return Ok(None);
    };
    let slugs: Vec<String> = serde_json::from_value(value.clone())
        .map_err(|e| CoreError::Internal(anyhow::anyhow!("malformed api key scopes: {e}")))?;
    Ok(Some(
        slugs
            .iter()
            .filter_map(|s| Permission::from_slug(s))
            .collect(),
    ))
}

fn to_dto(model: entity::api_key::Model) -> CoreResult<ApiKeyDto> {
    let scopes = parse_scopes(&model)?;
    Ok(ApiKeyDto {
        id: model.id,
        name: model.name,
        prefix: model.prefix,
        scopes,
        expires_at: model.expires_at,
        last_used_at: model.last_used_at,
        created_at: model.created_at,
        revoked_at: model.revoked_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issued_secrets_carry_the_prefix_and_are_unique() {
        let a = format!("{TOKEN_PREFIX}{}", generate_secret());
        let b = format!("{TOKEN_PREFIX}{}", generate_secret());
        assert!(a.starts_with(TOKEN_PREFIX));
        assert_ne!(a, b);
        // The display prefix must not leak enough to matter.
        let shown: String = a.chars().take(PREFIX_LEN).collect();
        assert_eq!(shown.len(), PREFIX_LEN);
        assert!(a.len() > PREFIX_LEN * 4);
    }

    #[test]
    fn a_name_is_trimmed_and_bounded() {
        assert_eq!(validate_name("  Claude Desktop  ").unwrap(), "Claude Desktop");
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"x".repeat(MAX_NAME_LEN + 1)).is_err());
    }

    #[test]
    fn unknown_scope_slugs_are_dropped_not_fatal() {
        let row = entity::api_key::Model {
            id: 1,
            user_id: 1,
            name: "k".into(),
            token_hash: "h".into(),
            prefix: "orl_abcd".into(),
            scopes: Some(serde_json::json!(["forms:read", "forms:teleport"])),
            expires_at: None,
            last_used_at: None,
            created_at: Utc::now(),
            revoked_at: None,
        };
        let scopes = parse_scopes(&row).unwrap().unwrap();
        assert_eq!(scopes, vec![Permission::FormsRead]);
    }
}
