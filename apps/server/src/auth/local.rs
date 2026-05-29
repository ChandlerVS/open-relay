use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize, ToSchema)]
pub struct LoginResponse {
    pub token: String,
}

#[utoipa::path(
    post,
    path = "/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "JWT issued", body = LoginResponse),
        (status = 501, description = "User entity not yet implemented"),
    )
)]
pub async fn login(Json(_req): Json<LoginRequest>) -> AppResult<Json<LoginResponse>> {
    // TODO(users): look up user by email, verify argon2 hash, then
    // `issue_jwt(state.auth_keys, &Claims { sub: user.id.to_string(), exp: … })`.
    Err(AppError::NotImplemented(
        "local login requires the user entity",
    ))
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(login))
}
