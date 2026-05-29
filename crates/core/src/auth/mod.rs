//! Authentication primitives: JWT key material, claims, and a pluggable
//! `Provider` trait for SSO/OAuth. Pure domain — nothing in here knows about
//! HTTP. The server crate layers Axum extractors on top.

pub mod provider;

use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::CoreError;
use crate::permissions::Permission;
use crate::rbac::RoleSummary;
use crate::users::UserDto;

/// JWT lifetime, in seconds. 24 hours.
pub const JWT_TTL_SECONDS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone)]
pub struct AuthKeys {
    pub encoding: EncodingKey,
    pub decoding: DecodingKey,
}

impl AuthKeys {
    pub fn from_secret(secret: &[u8]) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
}

pub fn issue_jwt(keys: &AuthKeys, claims: &Claims) -> Result<String, CoreError> {
    encode(&Header::default(), claims, &keys.encoding)
        .map_err(|e| CoreError::Internal(anyhow::anyhow!(e)))
}

/// Issue a JWT for a user with the standard TTL.
pub fn issue_for_user(keys: &AuthKeys, user: &entity::user::Model) -> Result<String, CoreError> {
    let exp = (chrono::Utc::now().timestamp() + JWT_TTL_SECONDS) as usize;
    let claims = Claims {
        sub: user.id.to_string(),
        exp,
    };
    issue_jwt(keys, &claims)
}

/// Verify a JWT signed by our key and return its claims.
pub fn verify_jwt(keys: &AuthKeys, token: &str) -> Result<Claims, CoreError> {
    decode::<Claims>(token, &keys.decoding, &Validation::default())
        .map(|d| d.claims)
        .map_err(|_| CoreError::Unauthorized)
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserDto,
}

/// Session-shape response for `/auth/me`. Flat permission set is what the
/// frontend's `usePermissions` hook consumes; `roles` provides the role
/// badges in the UI. Refresh on window focus so an admin's permission
/// changes propagate without forcing a re-login.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MeResponse {
    pub user: UserDto,
    pub permissions: Vec<Permission>,
    pub roles: Vec<RoleSummary>,
}
