//! Backend instance management routes.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use open_relay_core::backend::BackendKindInfo;
use open_relay_core::backends::{
    BackendInstanceDto, BackendInstanceInUse, BackendInstanceList, NewBackendInstance,
    UpdateBackendInstance, service,
};
use open_relay_core::error::CoreError;
use open_relay_core::permissions::Permission;
use sea_orm::TransactionTrait;
use serde_json::json;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::auth::permissions::require_permission;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "",
    tag = "backends",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Backend instances", body = BackendInstanceList),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn list_backends(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<BackendInstanceList>> {
    require_permission(&state, claims, Permission::BackendsRead).await?;
    let list = service::list(&state.db).await?;
    Ok(Json(list))
}

#[utoipa::path(
    get,
    path = "/kinds",
    tag = "backends",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Backend kind catalogue", body = [BackendKindInfo]),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn list_backend_kinds(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<Vec<BackendKindInfo>>> {
    require_permission(&state, claims, Permission::BackendsRead).await?;
    Ok(Json(state.backends.kinds()))
}

#[utoipa::path(
    get,
    path = "/{id}",
    tag = "backends",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Backend instance id")),
    responses(
        (status = 200, description = "Backend instance", body = BackendInstanceDto),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Backend instance not found"),
    )
)]
pub async fn get_backend(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<Json<BackendInstanceDto>> {
    require_permission(&state, claims, Permission::BackendsRead).await?;
    let row = service::find_by_id(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("backend instance not found".into()))?;
    Ok(Json(row.into()))
}

#[utoipa::path(
    post,
    path = "",
    tag = "backends",
    security(("bearer" = [])),
    request_body = NewBackendInstance,
    responses(
        (status = 201, description = "Backend instance created", body = BackendInstanceDto),
        (status = 400, description = "Validation failed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn create_backend(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(input): Json<NewBackendInstance>,
) -> AppResult<impl IntoResponse> {
    require_permission(&state, claims, Permission::BackendsWrite).await?;
    let registry = state.backends.clone();
    let row = state
        .db
        .transaction::<_, entity::backend_instance::Model, CoreError>(|tx| {
            Box::pin(async move { service::create(tx, &registry, input).await })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok((StatusCode::CREATED, Json(BackendInstanceDto::from(row))))
}

#[utoipa::path(
    patch,
    path = "/{id}",
    tag = "backends",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Backend instance id")),
    request_body = UpdateBackendInstance,
    responses(
        (status = 200, description = "Backend instance updated", body = BackendInstanceDto),
        (status = 400, description = "Validation failed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Backend instance not found"),
    )
)]
pub async fn update_backend(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
    Json(input): Json<UpdateBackendInstance>,
) -> AppResult<Json<BackendInstanceDto>> {
    require_permission(&state, claims, Permission::BackendsWrite).await?;
    let registry = state.backends.clone();
    let row = state
        .db
        .transaction::<_, entity::backend_instance::Model, CoreError>(|tx| {
            Box::pin(async move { service::update(tx, &registry, id, input).await })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok(Json(row.into()))
}

#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "backends",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Backend instance id")),
    responses(
        (status = 204, description = "Backend instance deleted"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Backend instance not found"),
        (status = 409, description = "Still referenced by forms", body = BackendInstanceInUse),
    )
)]
pub async fn delete_backend(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    require_permission(&state, claims, Permission::BackendsDelete).await?;
    let result = state
        .db
        .transaction::<_, (), CoreError>(|tx| {
            Box::pin(async move { service::delete(tx, id).await })
        })
        .await;
    match result {
        Ok(()) => Ok(StatusCode::NO_CONTENT.into_response()),
        Err(sea_orm::TransactionError::Connection(db)) => Err(AppError::Db(db)),
        Err(sea_orm::TransactionError::Transaction(CoreError::Conflict(payload))) => {
            // Service serializes a `BackendInstanceInUse` payload into the
            // conflict message. Lift it back into the response body so the
            // admin sees the referencing forms.
            let body: serde_json::Value = serde_json::from_str(&payload)
                .unwrap_or_else(|_| json!({ "error": payload }));
            Ok((StatusCode::CONFLICT, Json(body)).into_response())
        }
        Err(sea_orm::TransactionError::Transaction(core)) => Err(core.into()),
    }
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_backends, create_backend))
        .routes(routes!(list_backend_kinds))
        .routes(routes!(get_backend, update_backend, delete_backend))
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(db) => AppError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core.into(),
    }
}
