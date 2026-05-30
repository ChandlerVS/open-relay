//! User persistence + password hashing.
//!
//! All functions take `&impl ConnectionTrait` so callers can pass either
//! a `DatabaseConnection` or a `DatabaseTransaction`.

use anyhow::anyhow;
use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect,
};

use super::{ChangePassword, ListQuery, NewUser, UpdateUser, UserDto, UserList, UserSelectOption};
use crate::auth::provider::VerifiedIdentity;
use crate::error::{CoreError, CoreResult};
use crate::external_identity::service as external_identity_service;
use crate::forms::service as forms_service;
use crate::rbac::service as rbac_service;
use crate::submissions::service as submissions_service;

const MIN_PASSWORD_LEN: usize = 12;
const MAX_DISPLAY_NAME_LEN: usize = 255;
const DEFAULT_LIST_LIMIT: u32 = 50;
const MAX_LIST_LIMIT: u32 = 200;

pub fn hash_password(plain: &str) -> CoreResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| CoreError::Internal(anyhow!("argon2 hash failed: {e}")))
}

pub fn verify_password(hash: &str, plain: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok()
}

pub fn validate_email(email: &str) -> CoreResult<()> {
    if !looks_like_email(email) {
        return Err(CoreError::BadRequest("invalid email".into()));
    }
    Ok(())
}

pub fn validate_password(password: &str) -> CoreResult<()> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(CoreError::BadRequest(format!(
            "password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    Ok(())
}

/// `Some(name)` requires 1..=MAX_DISPLAY_NAME_LEN after trim. `None` is fine.
pub fn validate_display_name(name: Option<&str>) -> CoreResult<()> {
    if let Some(name) = name {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.len() > MAX_DISPLAY_NAME_LEN {
            return Err(CoreError::BadRequest(format!(
                "display_name must be 1..={MAX_DISPLAY_NAME_LEN} characters"
            )));
        }
    }
    Ok(())
}

pub fn validate_new_user(input: &NewUser) -> CoreResult<()> {
    validate_email(input.email.trim())?;
    validate_password(&input.password)?;
    validate_display_name(input.display_name.as_deref())?;
    Ok(())
}

fn looks_like_email(s: &str) -> bool {
    let Some(at) = s.find('@') else {
        return false;
    };
    let (local, domain) = s.split_at(at);
    let domain = &domain[1..]; // strip '@'
    !local.is_empty()
        && !local.contains(char::is_whitespace)
        && domain.contains('.')
        && !domain.contains(char::is_whitespace)
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

pub async fn find_by_email<C: ConnectionTrait>(
    conn: &C,
    email: &str,
) -> CoreResult<Option<entity::user::Model>> {
    Ok(entity::user::Entity::find()
        .filter(entity::user::Column::Email.eq(email))
        .one(conn)
        .await?)
}

pub async fn find_by_id<C: ConnectionTrait>(
    conn: &C,
    id: i32,
) -> CoreResult<Option<entity::user::Model>> {
    Ok(entity::user::Entity::find_by_id(id).one(conn).await?)
}

pub async fn create_user<C: ConnectionTrait>(
    conn: &C,
    input: NewUser,
) -> CoreResult<entity::user::Model> {
    validate_new_user(&input)?;
    let email = input.email.trim().to_string();
    if find_by_email(conn, &email).await?.is_some() {
        return Err(CoreError::Conflict("email already in use".into()));
    }
    let password_hash = hash_password(&input.password)?;
    let display_name = input
        .display_name
        .as_ref()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    let model = entity::user::ActiveModel {
        email: ActiveValue::Set(email),
        password_hash: ActiveValue::Set(Some(password_hash)),
        display_name: ActiveValue::Set(display_name),
        ..Default::default()
    };
    Ok(model.insert(conn).await?)
}

pub async fn list_users<C: ConnectionTrait>(conn: &C, q: &ListQuery) -> CoreResult<UserList> {
    let limit = q.limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT);
    let offset = q.offset.unwrap_or(0);
    let mut items: Vec<UserDto> = entity::user::Entity::find()
        .order_by_asc(entity::user::Column::Id)
        .limit(limit as u64)
        .offset(offset as u64)
        .all(conn)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    populate_roles(conn, &mut items).await?;
    let total = entity::user::Entity::find().count(conn).await?;
    Ok(UserList {
        items,
        total,
        limit,
        offset,
    })
}

/// Enrich a slice of `UserDto`s with their role assignments. Single batched
/// fetch; safe to call with an empty slice.
pub async fn populate_roles<C: ConnectionTrait>(
    conn: &C,
    items: &mut [UserDto],
) -> CoreResult<()> {
    if items.is_empty() {
        return Ok(());
    }
    let ids: Vec<i32> = items.iter().map(|u| u.id).collect();
    let mut by_user = rbac_service::roles_for_users(conn, &ids).await?;
    for item in items {
        item.roles = by_user.remove(&item.id).unwrap_or_default();
    }
    Ok(())
}

/// Single-user enrichment helper used by `/auth/me` and the user-detail
/// endpoint.
pub async fn dto_with_roles<C: ConnectionTrait>(
    conn: &C,
    user: entity::user::Model,
) -> CoreResult<UserDto> {
    let roles = rbac_service::roles_for_user(conn, user.id).await?;
    let mut dto: UserDto = user.into();
    dto.roles = roles;
    Ok(dto)
}

pub async fn select_list<C: ConnectionTrait>(conn: &C) -> CoreResult<Vec<UserSelectOption>> {
    let mut rows: Vec<UserSelectOption> = entity::user::Entity::find()
        .order_by_asc(entity::user::Column::DisplayName)
        .order_by_asc(entity::user::Column::Email)
        .all(conn)
        .await?
        .into_iter()
        .map(|m| {
            let label = m.display_name.clone().unwrap_or_else(|| {
                m.email
                    .split_once('@')
                    .map(|(local, _)| local.to_string())
                    .unwrap_or_else(|| m.email.clone())
            });
            UserSelectOption { id: m.id, label }
        })
        .collect();
    // Stable secondary sort by label so the dropdown reads naturally regardless
    // of how MySQL ordered NULL display_names against trailing rows.
    rows.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    Ok(rows)
}

pub async fn update_user<C: ConnectionTrait>(
    conn: &C,
    id: i32,
    input: UpdateUser,
) -> CoreResult<entity::user::Model> {
    let existing = find_by_id(conn, id)
        .await?
        .ok_or_else(|| CoreError::NotFound("user not found".into()))?;
    let mut active: entity::user::ActiveModel = existing.clone().into();

    if let Some(email_raw) = input.email {
        let email = email_raw.trim().to_string();
        validate_email(&email)?;
        if email != existing.email {
            if let Some(other) = find_by_email(conn, &email).await? {
                if other.id != id {
                    return Err(CoreError::Conflict("email already in use".into()));
                }
            }
            active.email = ActiveValue::Set(email);
        }
    }

    if let Some(name_raw) = input.display_name {
        let trimmed = name_raw.trim();
        if trimmed.is_empty() {
            active.display_name = ActiveValue::Set(None);
        } else {
            validate_display_name(Some(trimmed))?;
            active.display_name = ActiveValue::Set(Some(trimmed.to_string()));
        }
    }

    Ok(active.update(conn).await?)
}

pub async fn change_password<C: ConnectionTrait>(
    conn: &C,
    id: i32,
    input: ChangePassword,
) -> CoreResult<()> {
    validate_password(&input.password)?;
    let existing = find_by_id(conn, id)
        .await?
        .ok_or_else(|| CoreError::NotFound("user not found".into()))?;
    let hash = hash_password(&input.password)?;
    let mut active: entity::user::ActiveModel = existing.into();
    active.password_hash = ActiveValue::Set(Some(hash));
    active.update(conn).await?;
    Ok(())
}

/// Delete a user, enforcing both the self-delete guard and the
/// last-superadmin guard in core (rather than the HTTP layer) so non-HTTP
/// callers can't bypass them. Cleans up dependent `user_role` rows first
/// since FK cascades are managed in application code.
///
/// MUST be called inside a transaction — the last-superadmin check takes
/// an exclusive lock on the superadmin role row that only holds within a
/// tx.
pub async fn delete_user<C: ConnectionTrait>(
    conn: &C,
    actor_id: i32,
    target_id: i32,
    superadmin_role_id: i32,
) -> CoreResult<()> {
    if actor_id == target_id {
        return Err(CoreError::Forbidden(
            "cannot delete the currently authenticated user".into(),
        ));
    }
    if find_by_id(conn, target_id).await?.is_none() {
        return Err(CoreError::NotFound("user not found".into()));
    }
    let target_is_superadmin = entity::user_role::Entity::find()
        .filter(entity::user_role::Column::UserId.eq(target_id))
        .filter(entity::user_role::Column::RoleId.eq(superadmin_role_id))
        .one(conn)
        .await?
        .is_some();
    if target_is_superadmin {
        rbac_service::guard_last_superadmin(conn, target_id, superadmin_role_id).await?;
    }
    entity::user_role::Entity::delete_many()
        .filter(entity::user_role::Column::UserId.eq(target_id))
        .exec(conn)
        .await?;
    submissions_service::delete_for_owner(conn, target_id).await?;
    forms_service::delete_for_owner(conn, target_id).await?;
    external_identity_service::delete_for_user(conn, target_id).await?;
    let res = entity::user::Entity::delete_by_id(target_id)
        .exec(conn)
        .await?;
    if res.rows_affected == 0 {
        return Err(CoreError::NotFound("user not found".into()));
    }
    Ok(())
}

/// Resolve an OAuth identity to a local user, creating one if necessary.
///
/// Branches:
/// 1. An existing `external_identity` row for `(provider_config_id, subject)`
///    → load and return that user.
/// 2. No identity row, but `verified.email` matches an existing user → link
///    the identity to that user (first-time sign-in for a known account).
/// 3. Otherwise → insert a new user with `password_hash = None`, link the
///    identity, and assign `default_role_id` if one was configured.
///
/// MUST be called inside a transaction.
pub async fn find_or_create_via_oauth<C: ConnectionTrait>(
    conn: &C,
    provider_config_id: i32,
    verified: &VerifiedIdentity,
    default_role_id: Option<i32>,
    superadmin_role_id: i32,
) -> CoreResult<entity::user::Model> {
    if let Some(existing) =
        external_identity_service::find_by_provider_subject(conn, provider_config_id, &verified.subject)
            .await?
    {
        return find_by_id(conn, existing.user_id)
            .await?
            .ok_or_else(|| CoreError::Internal(anyhow!("orphaned external_identity row")));
    }

    let email_normalized = verified
        .email
        .as_deref()
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty());

    if let Some(email) = &email_normalized {
        if let Some(user) = find_by_email(conn, email).await? {
            external_identity_service::link_to_user(
                conn,
                user.id,
                provider_config_id,
                &verified.subject,
                Some(email),
            )
            .await?;
            return Ok(user);
        }
    }

    let email = email_normalized.clone().ok_or_else(|| {
        CoreError::BadRequest("OAuth provider did not return an email; cannot auto-create user".into())
    })?;
    if find_by_email(conn, &email).await?.is_some() {
        return Err(CoreError::Conflict("email already in use".into()));
    }
    let new_user = entity::user::ActiveModel {
        email: ActiveValue::Set(email.clone()),
        password_hash: ActiveValue::Set(None),
        display_name: ActiveValue::Set(None),
        ..Default::default()
    };
    let created = new_user.insert(conn).await?;
    external_identity_service::link_to_user(
        conn,
        created.id,
        provider_config_id,
        &verified.subject,
        Some(&email),
    )
    .await?;
    if let Some(role_id) = default_role_id {
        rbac_service::assign_roles_to_user(conn, created.id, &[role_id], superadmin_role_id).await?;
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_round_trip() {
        let h = hash_password("correct-horse-battery").unwrap();
        assert!(verify_password(&h, "correct-horse-battery"));
        assert!(!verify_password(&h, "wrong"));
    }

    #[test]
    fn email_validation() {
        assert!(looks_like_email("a@b.co"));
        assert!(!looks_like_email("not-an-email"));
        assert!(!looks_like_email("a@b"));
        assert!(!looks_like_email("@b.co"));
        assert!(!looks_like_email("a@.co"));
        assert!(!looks_like_email("a @b.co"));
    }

    #[test]
    fn validate_rejects_short_password() {
        let input = NewUser {
            email: "a@b.co".into(),
            password: "short".into(),
            display_name: None,
            role_ids: Vec::new(),
        };
        assert!(validate_new_user(&input).is_err());
    }
}
