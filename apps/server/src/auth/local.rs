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
    if !service::verify_password(&user.password_hash, &req.password) {
        return Err(AppError::Unauthorized);
    }
    let token = auth::issue_for_user(&state.auth_keys, &user)?;
    Ok(Json(LoginResponse {
        token,
        user: user.into(),
    }))
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(login))
}
