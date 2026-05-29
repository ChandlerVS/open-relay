use axum::Json;
use axum::extract::State;
use open_relay_core::users::{UserDto, service};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/me",
    tag = "auth",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Current user", body = UserDto),
        (status = 401, description = "Missing or invalid token"),
    )
)]
pub async fn me(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<UserDto>> {
    let id: i32 = claims.sub.parse().map_err(|_| AppError::Unauthorized)?;
    let user = service::find_by_id(&state.db, id)
        .await?
        .ok_or(AppError::Unauthorized)?;
    Ok(Json(user.into()))
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(me))
}
