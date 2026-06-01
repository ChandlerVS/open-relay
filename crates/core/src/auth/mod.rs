//! Authentication primitives: JWT key material, claims, and a pluggable
//! `Provider` trait for SSO/OAuth. Pure domain — nothing in here knows about
//! HTTP. The server crate layers Axum extractors on top.

pub mod provider;
pub mod refresh;

use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::CoreError;
use crate::permissions::Permission;
use crate::rbac::RoleSummary;
use crate::users::UserDto;

/// Access-token (JWT) lifetime, in seconds. Deliberately short: the access
/// token is stateless, so revocation (logout, password change, demotion) only
/// takes full effect once it expires. The refresh token carries the long-lived,
/// server-revocable session. 15 minutes.
pub const ACCESS_TTL_SECONDS: i64 = 15 * 60;

/// Refresh-token lifetime, in seconds. 30 days.
pub const REFRESH_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone)]
pub struct AuthKeys {
    pub encoding: EncodingKey,
    pub decoding: DecodingKey,
    /// Raw secret bytes, kept so HMAC-based helpers (e.g. the OAuth state
    /// cookie signer) can derive sub-keys without taking on a second secret.
    secret: Vec<u8>,
}

impl AuthKeys {
    pub fn from_secret(secret: &[u8]) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            secret: secret.to_vec(),
        }
    }

    pub fn hmac_secret(&self) -> &[u8] {
        &self.secret
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

/// Issue an access JWT for a user with the short access TTL.
pub fn issue_for_user(keys: &AuthKeys, user: &entity::user::Model) -> Result<String, CoreError> {
    let exp = (chrono::Utc::now().timestamp() + ACCESS_TTL_SECONDS) as usize;
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
    /// Short-lived access JWT (sent as `Authorization: Bearer`).
    pub token: String,
    /// Opaque refresh secret — exchanged at `/auth/refresh` for a new access
    /// token. Returned only here and on refresh; never retrievable afterward.
    pub refresh_token: String,
    pub user: UserDto,
}

/// Result of a successful `/auth/refresh` rotation: a fresh access token plus
/// the rotated refresh secret (the presented one is now revoked).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TokenPair {
    pub token: String,
    pub refresh_token: String,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct RefreshRequest {
    pub refresh_token: String,
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
