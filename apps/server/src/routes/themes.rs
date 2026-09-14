//! Theme routes — CRUD for reusable form themes.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use open_relay_core::error::CoreError;
use open_relay_core::permissions::Permission;
use open_relay_core::themes::{NewTheme, ThemeDto, ThemeList, UpdateTheme, service};
use sea_orm::TransactionTrait;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::auth::permissions::require_permission;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "",
    tag = "themes",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Themes", body = ThemeList),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn list_themes(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<ThemeList>> {
    require_permission(&state, claims, Permission::ThemesRead).await?;
    Ok(Json(service::list(&state.db).await?))
}

#[utoipa::path(
    get,
    path = "/{id}",
    tag = "themes",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Theme id")),
    responses(
        (status = 200, description = "Theme", body = ThemeDto),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Theme not found"),
    )
)]
pub async fn get_theme(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<Json<ThemeDto>> {
    require_permission(&state, claims, Permission::ThemesRead).await?;
    Ok(Json(service::get(&state.db, id).await?))
}

#[utoipa::path(
    post,
    path = "",
    tag = "themes",
    security(("bearer" = [])),
    request_body = NewTheme,
    responses(
        (status = 201, description = "Theme created", body = ThemeDto),
        (status = 400, description = "Validation failed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn create_theme(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(input): Json<NewTheme>,
) -> AppResult<impl IntoResponse> {
    require_permission(&state, claims, Permission::ThemesWrite).await?;
    // A transaction because making a theme the default also unsets the old one.
    let dto = state
        .db
        .transaction::<_, ThemeDto, CoreError>(|tx| {
            Box::pin(async move { service::create(tx, input).await })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok((StatusCode::CREATED, Json(dto)))
}

#[utoipa::path(
    patch,
    path = "/{id}",
    tag = "themes",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Theme id")),
    request_body = UpdateTheme,
    responses(
        (status = 200, description = "Theme updated", body = ThemeDto),
        (status = 400, description = "Validation failed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Theme not found"),
    )
)]
pub async fn update_theme(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
    Json(input): Json<UpdateTheme>,
) -> AppResult<Json<ThemeDto>> {
    require_permission(&state, claims, Permission::ThemesWrite).await?;
    let dto = state
        .db
        .transaction::<_, ThemeDto, CoreError>(|tx| {
            Box::pin(async move { service::update(tx, id, input).await })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok(Json(dto))
}

#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "themes",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Theme id")),
    responses(
        (status = 204, description = "Theme deleted; forms that used it fall back to the default theme"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Theme not found"),
    )
)]
pub async fn delete_theme(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    require_permission(&state, claims, Permission::ThemesDelete).await?;
    state
        .db
        .transaction::<_, (), CoreError>(|tx| {
            Box::pin(async move { service::delete(tx, id).await })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_themes, create_theme))
        .routes(routes!(get_theme, update_theme, delete_theme))
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(db) => AppError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core.into(),
    }
}
