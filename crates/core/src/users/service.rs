//! User persistence + password hashing.
//!
//! All functions take `&impl ConnectionTrait` so callers can pass either
//! a `DatabaseConnection` or a `DatabaseTransaction`.

use anyhow::anyhow;
use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};

use super::NewUser;
use crate::error::{CoreError, CoreResult};

const MIN_PASSWORD_LEN: usize = 12;
const MAX_DISPLAY_NAME_LEN: usize = 255;

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

pub fn validate_new_user(input: &NewUser) -> CoreResult<()> {
    let email = input.email.trim();
    if !looks_like_email(email) {
        return Err(CoreError::BadRequest("invalid email".into()));
    }
    if input.password.len() < MIN_PASSWORD_LEN {
        return Err(CoreError::BadRequest(format!(
            "password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    if let Some(name) = &input.display_name {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.len() > MAX_DISPLAY_NAME_LEN {
            return Err(CoreError::BadRequest(format!(
                "display_name must be 1..={MAX_DISPLAY_NAME_LEN} characters"
            )));
        }
    }
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

pub async fn create_user<C: ConnectionTrait>(
    conn: &C,
    input: NewUser,
) -> CoreResult<entity::user::Model> {
    validate_new_user(&input)?;
    let password_hash = hash_password(&input.password)?;
    let display_name = input
        .display_name
        .as_ref()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    let model = entity::user::ActiveModel {
        email: ActiveValue::Set(input.email.trim().to_string()),
        password_hash: ActiveValue::Set(password_hash),
        display_name: ActiveValue::Set(display_name),
        ..Default::default()
    };
    Ok(model.insert(conn).await?)
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
