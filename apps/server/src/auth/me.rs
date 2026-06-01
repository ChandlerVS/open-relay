use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use open_relay_core::auth::MeResponse;
use open_relay_core::error::CoreError;
use open_relay_core::rbac::service as rbac_service;
use open_relay_core::users::{ChangeOwnPassword, service};
use sea_orm::TransactionTrait;
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

#[utoipa::path(
    post,
    path = "/password",
    tag = "auth",
    security(("bearer" = [])),
    request_body = ChangeOwnPassword,
    responses(
        (status = 204, description = "Password changed; other sessions revoked"),
        (status = 400, description = "Validation failed / no local password"),
        (status = 401, description = "Missing/invalid token or wrong current password"),
    )
)]
pub async fn change_own_password(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(input): Json<ChangeOwnPassword>,
) -> AppResult<impl IntoResponse> {
    let authz = authenticated_user(claims)?;
    let user_id = authz.id;
    state
        .db
        .transaction::<_, (), CoreError>(|tx| {
            Box::pin(async move { service::change_own_password(tx, user_id, input).await })
        })
        .await
        .map_err(|e| match e {
            sea_orm::TransactionError::Connection(db) => AppError::Db(db),
            sea_orm::TransactionError::Transaction(core) => core.into(),
        })?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(me))
        .routes(routes!(change_own_password))
}
