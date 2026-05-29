//! Role management routes + the static permission catalogue.
//!
//! Permissions themselves are code-defined (`open_relay_core::permissions`);
//! these endpoints expose the role rows that bundle them and let admins
//! assign permission sets through the UI. `GET /permissions` is the
//! frontend's source of truth for rendering the role editor's checkboxes.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use open_relay_core::error::CoreError;
use open_relay_core::permissions::{self, Permission, PermissionInfo};
use open_relay_core::rbac::{NewRole, RoleDto, RoleSummary, UpdateRole, service as rbac_service};
use sea_orm::TransactionTrait;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::auth::permissions::{authenticated_user, require_permission};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "",
    tag = "roles",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Role list", body = [RoleDto]),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn list_roles(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<Vec<RoleDto>>> {
    require_permission(&state, claims, Permission::RolesRead).await?;
    let rows = rbac_service::list_roles(&state.db).await?;
    Ok(Json(rows))
}

#[utoipa::path(
    post,
    path = "",
    tag = "roles",
    security(("bearer" = [])),
    request_body = NewRole,
    responses(
        (status = 201, description = "Role created", body = RoleDto),
        (status = 400, description = "Validation failed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 409, description = "Role name already in use"),
    )
)]
pub async fn create_role(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(input): Json<NewRole>,
) -> AppResult<impl IntoResponse> {
    require_permission(&state, claims, Permission::RolesWrite).await?;
    let role = state
        .db
        .transaction::<_, RoleDto, CoreError>(|tx| {
            Box::pin(async move { rbac_service::create_role(tx, input).await })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok((StatusCode::CREATED, Json(role)))
}

#[utoipa::path(
    get,
    path = "/select-list",
    tag = "roles",
    operation_id = "roles_select_list",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Dropdown-friendly role list", body = [RoleSummary]),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn select_list(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<Vec<RoleSummary>>> {
    require_permission(&state, claims, Permission::RolesRead).await?;
    let rows = rbac_service::select_list_summary(&state.db).await?;
    Ok(Json(rows))
}

#[utoipa::path(
    get,
    path = "/{id}",
    tag = "roles",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Role id")),
    responses(
        (status = 200, description = "Role detail", body = RoleDto),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Role not found"),
    )
)]
pub async fn get_role(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<Json<RoleDto>> {
    require_permission(&state, claims, Permission::RolesRead).await?;
    let role = rbac_service::get_role(&state.db, id).await?;
    Ok(Json(role))
}

#[utoipa::path(
    patch,
    path = "/{id}",
    tag = "roles",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Role id")),
    request_body = UpdateRole,
    responses(
        (status = 200, description = "Role updated", body = RoleDto),
        (status = 400, description = "Validation failed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission or system role"),
        (status = 404, description = "Role not found"),
        (status = 409, description = "Role name already in use"),
    )
)]
pub async fn update_role(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
    Json(input): Json<UpdateRole>,
) -> AppResult<Json<RoleDto>> {
    require_permission(&state, claims, Permission::RolesWrite).await?;
    let role = state
        .db
        .transaction::<_, RoleDto, CoreError>(|tx| {
            Box::pin(async move { rbac_service::update_role(tx, id, input).await })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok(Json(role))
}

#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "roles",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Role id")),
    responses(
        (status = 204, description = "Role deleted"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission or system role"),
        (status = 404, description = "Role not found"),
    )
)]
pub async fn delete_role(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<impl IntoResponse> {
    require_permission(&state, claims, Permission::RolesDelete).await?;
    state
        .db
        .transaction::<_, (), CoreError>(|tx| {
            Box::pin(async move { rbac_service::delete_role(tx, id).await })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "",
    tag = "roles",
    operation_id = "list_permissions",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Permission catalogue", body = [PermissionInfo]),
        (status = 401, description = "Missing or invalid token"),
    )
)]
pub async fn list_permissions(
    AuthUser(claims): AuthUser,
) -> AppResult<Json<Vec<PermissionInfo>>> {
    // Authenticated session is enough — the role editor needs the catalogue
    // whenever a user can reach it, and the catalogue is not secret.
    authenticated_user(claims)?;
    Ok(Json(permissions::catalog()))
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_roles, create_role))
        .routes(routes!(select_list))
        .routes(routes!(get_role, update_role, delete_role))
}

pub fn permissions_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_permissions))
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(db) => AppError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core.into(),
    }
}
