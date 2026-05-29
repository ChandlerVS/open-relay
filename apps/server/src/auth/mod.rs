//! Axum-side authentication wiring.
//!
//! Pure auth primitives — `AuthKeys`, `Claims`, JWT issuance, the `Provider`
//! trait — live in `open_relay_core::auth`. This module adds the bits that
//! only make sense in an HTTP server: the `AuthUser` extractor and the auth
//! sub-router.

pub mod local;
pub mod me;
pub mod permissions;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use open_relay_core::auth::{Claims, verify_jwt};
use utoipa_axum::router::OpenApiRouter;

use crate::error::AppError;
use crate::state::AppState;

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
        let claims = verify_jwt(&state.auth_keys, token).map_err(AppError::from)?;
        Ok(AuthUser(claims))
    }
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .merge(local::router())
        .merge(me::router())
}
