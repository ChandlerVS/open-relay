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

use super::{ChangePassword, ListQuery, NewUser, UpdateUser, UserList, UserSelectOption};
use crate::error::{CoreError, CoreResult};

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
        password_hash: ActiveValue::Set(password_hash),
        display_name: ActiveValue::Set(display_name),
        ..Default::default()
    };
    Ok(model.insert(conn).await?)
}

pub async fn list_users<C: ConnectionTrait>(conn: &C, q: &ListQuery) -> CoreResult<UserList> {
    let limit = q.limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT);
    let offset = q.offset.unwrap_or(0);
    let items = entity::user::Entity::find()
        .order_by_asc(entity::user::Column::Id)
        .limit(limit as u64)
        .offset(offset as u64)
        .all(conn)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let total = entity::user::Entity::find().count(conn).await?;
    Ok(UserList {
        items,
        total,
        limit,
        offset,
    })
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
    active.password_hash = ActiveValue::Set(hash);
    active.update(conn).await?;
    Ok(())
}

pub async fn delete_user<C: ConnectionTrait>(conn: &C, id: i32) -> CoreResult<()> {
    let res = entity::user::Entity::delete_by_id(id).exec(conn).await?;
    if res.rows_affected == 0 {
        return Err(CoreError::NotFound("user not found".into()));
    }
    Ok(())
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
        };
        assert!(validate_new_user(&input).is_err());
    }
}
