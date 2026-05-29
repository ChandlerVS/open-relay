use axum::Json;
use axum::extract::State;
use open_relay_core::auth::MeResponse;
use open_relay_core::rbac::service as rbac_service;
use open_relay_core::users::service;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::auth::permissions::authenticated_user;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/me",
    tag = "auth",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Current session", body = MeResponse),
        (status = 401, description = "Missing or invalid token"),
    )
)]
pub async fn me(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<MeResponse>> {
    // `/auth/me` is intentionally not behind `require_permission` — a user
    // with zero permissions must still be able to load their session so the
    // SPA can render a useful "no access" screen instead of redirect-looping.
    let authz = authenticated_user(claims)?;
    let user = service::find_by_id(&state.db, authz.id)
        .await?
        .ok_or(AppError::Unauthorized)?;
    let roles = rbac_service::roles_for_user(&state.db, authz.id).await?;
    let permissions: Vec<_> = {
        let mut v: Vec<_> = rbac_service::load_user_permissions(&state.db, authz.id)
            .await?
            .into_iter()
            .collect();
        v.sort_by_key(|p| p.slug());
        v
    };
    let mut dto = open_relay_core::users::UserDto::from(user);
    dto.roles = roles.clone();
    Ok(Json(MeResponse {
        user: dto,
        permissions,
        roles,
    }))
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(me))
}
