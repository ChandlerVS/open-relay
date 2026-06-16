//! Sales rep directory routes — CRUD for the reusable rep resource.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use open_relay_core::error::CoreError;
use open_relay_core::permissions::Permission;
use open_relay_core::reps::{NewRep, RepDto, RepList, UpdateRep, service};
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
    tag = "reps",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Sales reps", body = RepList),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn list_reps(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<RepList>> {
    require_permission(&state, claims, Permission::RepsRead).await?;
    let list = service::list(&state.db).await?;
    Ok(Json(list))
}

#[utoipa::path(
    get,
    path = "/{id}",
    tag = "reps",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Sales rep id")),
    responses(
        (status = 200, description = "Sales rep", body = RepDto),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Sales rep not found"),
    )
)]
pub async fn get_rep(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<Json<RepDto>> {
    require_permission(&state, claims, Permission::RepsRead).await?;
    let row = service::find_by_id(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("sales rep not found".into()))?;
    Ok(Json(RepDto::from(row)))
}

#[utoipa::path(
    post,
    path = "",
    tag = "reps",
    security(("bearer" = [])),
    request_body = NewRep,
    responses(
        (status = 201, description = "Sales rep created", body = RepDto),
        (status = 400, description = "Validation failed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 409, description = "Key already in use"),
    )
)]
pub async fn create_rep(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(input): Json<NewRep>,
) -> AppResult<impl IntoResponse> {
    require_permission(&state, claims, Permission::RepsWrite).await?;
    let row = state
        .db
        .transaction::<_, entity::sales_rep::Model, CoreError>(|tx| {
            Box::pin(async move { service::create(tx, input).await })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok((StatusCode::CREATED, Json(RepDto::from(row))))
}

#[utoipa::path(
    patch,
    path = "/{id}",
    tag = "reps",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Sales rep id")),
    request_body = UpdateRep,
    responses(
        (status = 200, description = "Sales rep updated", body = RepDto),
        (status = 400, description = "Validation failed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Sales rep not found"),
        (status = 409, description = "Key already in use"),
    )
)]
pub async fn update_rep(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
    Json(input): Json<UpdateRep>,
) -> AppResult<Json<RepDto>> {
    require_permission(&state, claims, Permission::RepsWrite).await?;
    let row = state
        .db
        .transaction::<_, entity::sales_rep::Model, CoreError>(|tx| {
            Box::pin(async move { service::update(tx, id, input).await })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok(Json(RepDto::from(row)))
}

#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "reps",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Sales rep id")),
    responses(
        (status = 204, description = "Sales rep deleted"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Sales rep not found"),
    )
)]
pub async fn delete_rep(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    require_permission(&state, claims, Permission::RepsDelete).await?;
    state
        .db
        .transaction::<_, (), CoreError>(|tx| Box::pin(async move { service::delete(tx, id).await }))
        .await
        .map_err(unwrap_tx)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_reps, create_rep))
        .routes(routes!(get_rep, update_rep, delete_rep))
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(db) => AppError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core.into(),
    }
}
