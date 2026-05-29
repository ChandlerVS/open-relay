//! Authentication: local JWT issuance + a `Provider` trait for SSO/OAuth.
//!
//! The local flow is wired through `local::router()`; OAuth providers register
//! against [`provider::ProviderRegistry`] at startup. No concrete providers
//! ship in the skeleton — the trait is the extension point.

pub mod local;
pub mod provider;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use utoipa_axum::router::OpenApiRouter;

use crate::error::AppError;
use crate::state::AppState;

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

pub fn issue_jwt(keys: &AuthKeys, claims: &Claims) -> Result<String, AppError> {
    encode(&Header::default(), claims, &keys.encoding)
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))
}

/// Axum extractor — present a `Bearer <jwt>` header signed by our key.
pub struct AuthUser(pub Claims);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .ok_or(AppError::Unauthorized)?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or(AppError::Unauthorized)?;
        let data = decode::<Claims>(token, &state.auth_keys.decoding, &Validation::default())
            .map_err(|_| AppError::Unauthorized)?;
        Ok(AuthUser(data.claims))
    }
}

pub fn router() -> OpenApiRouter<AppState> {
    local::router()
}
