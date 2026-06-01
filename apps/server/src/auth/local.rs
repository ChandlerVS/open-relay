use axum::Json;
use axum::extract::State;
use open_relay_core::auth::{self, LoginRequest, LoginResponse};
use open_relay_core::users::service;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[utoipa::path(
    post,
    path = "/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "JWT issued", body = LoginResponse),
        (status = 401, description = "Invalid credentials"),
    )
)]
pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<LoginResponse>> {
    let user = service::find_by_email(&state.db, req.email.trim())
        .await?
        .ok_or(AppError::Unauthorized)?;
    let Some(hash) = user.password_hash.as_deref() else {
        return Err(AppError::Unauthorized);
    };
    if !service::verify_password(hash, &req.password) {
        return Err(AppError::Unauthorized);
    }
    let token = auth::issue_for_user(&state.auth_keys, &user)?;
    let refresh_token = auth::refresh::issue(&state.db, user.id).await?;
    Ok(Json(LoginResponse {
        token,
        refresh_token,
        user: user.into(),
    }))
}

pub fn router() -> OpenApiRouter<AppState> {
    // Per-IP rate limit on the login endpoint — online brute force / account
    // enumeration defense. Scoped here so it only throttles `/auth/login`.
    OpenApiRouter::new()
        .routes(routes!(login))
        .layer(crate::ratelimit::login_layer())
}
