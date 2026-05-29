//! User management routes — CRUD plus a lightweight `select-list` for
//! dropdowns.
//!
//! Every endpoint is gated by a code-defined permission via
//! `require_permission`. Mutations that touch role assignments
//! additionally require `roles:assign` — the wire payload accepts
//! `role_ids` on `NewUser`/`UpdateUser`, and the handler propagates it
//! inside the same transaction as the user mutation.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use open_relay_core::error::CoreError;
use open_relay_core::permissions::Permission;
use open_relay_core::rbac::service as rbac_service;
use open_relay_core::users::{
    ChangePassword, ListQuery, NewUser, UpdateUser, UserDto, UserList, UserSelectOption, service,
};
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
    tag = "users",
    security(("bearer" = [])),
    params(ListQuery),
    responses(
        (status = 200, description = "Paginated users", body = UserList),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn list_users(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<UserList>> {
    require_permission(&state, claims, Permission::UsersRead).await?;
    let list = service::list_users(&state.db, &q).await?;
    Ok(Json(list))
}

#[utoipa::path(
    get,
    path = "/select-list",
    tag = "users",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Dropdown-friendly user list", body = [UserSelectOption]),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn select_list(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<Vec<UserSelectOption>>> {
    require_permission(&state, claims, Permission::UsersRead).await?;
    let rows = service::select_list(&state.db).await?;
    Ok(Json(rows))
}

#[utoipa::path(
    get,
    path = "/{id}",
    tag = "users",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "User id")),
    responses(
        (status = 200, description = "User", body = UserDto),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "User not found"),
    )
)]
pub async fn get_user(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<Json<UserDto>> {
    require_permission(&state, claims, Permission::UsersRead).await?;
    let user = service::find_by_id(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".into()))?;
    let dto = service::dto_with_roles(&state.db, user).await?;
    Ok(Json(dto))
}

#[utoipa::path(
    post,
    path = "",
    tag = "users",
    security(("bearer" = [])),
    request_body = NewUser,
    responses(
        (status = 201, description = "User created", body = UserDto),
        (status = 400, description = "Validation failed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 409, description = "Email already in use"),
    )
)]
pub async fn create_user(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(input): Json<NewUser>,
) -> AppResult<impl IntoResponse> {
    let authz = require_permission(&state, claims, Permission::UsersWrite).await?;
    if !input.role_ids.is_empty() {
        require_permission(&state, authz.claims.clone(), Permission::RolesAssign).await?;
    }
    let superadmin_role_id = state.superadmin_role_id;
    let role_ids = input.role_ids.clone();
    let dto = state
        .db
        .transaction::<_, UserDto, CoreError>(|tx| {
            Box::pin(async move {
                let created = service::create_user(tx, NewUser {
                    role_ids: Vec::new(),
                    ..input
                })
                .await?;
                if !role_ids.is_empty() {
                    rbac_service::assign_roles_to_user(tx, created.id, &role_ids, superadmin_role_id)
                        .await?;
                }
                service::dto_with_roles(tx, created).await
            })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok((StatusCode::CREATED, Json(dto)))
}

#[utoipa::path(
    patch,
    path = "/{id}",
    tag = "users",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "User id")),
    request_body = UpdateUser,
    responses(
        (status = 200, description = "User updated", body = UserDto),
        (status = 400, description = "Validation failed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission or last-superadmin demotion"),
        (status = 404, description = "User not found"),
        (status = 409, description = "Email already in use"),
    )
)]
pub async fn update_user(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
    Json(input): Json<UpdateUser>,
) -> AppResult<Json<UserDto>> {
    let authz = require_permission(&state, claims, Permission::UsersWrite).await?;
    if input.role_ids.is_some() {
        require_permission(&state, authz.claims.clone(), Permission::RolesAssign).await?;
    }
    let superadmin_role_id = state.superadmin_role_id;
    let role_ids = input.role_ids.clone();
    let dto = state
        .db
        .transaction::<_, UserDto, CoreError>(|tx| {
            Box::pin(async move {
                let updated = service::update_user(tx, id, UpdateUser {
                    role_ids: None,
                    ..input
                })
                .await?;
                if let Some(ids) = role_ids {
                    rbac_service::assign_roles_to_user(tx, updated.id, &ids, superadmin_role_id)
                        .await?;
                }
                service::dto_with_roles(tx, updated).await
            })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok(Json(dto))
}

#[utoipa::path(
    post,
    path = "/{id}/password",
    tag = "users",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "User id")),
    request_body = ChangePassword,
    responses(
        (status = 204, description = "Password updated"),
        (status = 400, description = "Validation failed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "User not found"),
    )
)]
pub async fn change_password(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
    Json(input): Json<ChangePassword>,
) -> AppResult<impl IntoResponse> {
    require_permission(&state, claims, Permission::UsersWrite).await?;
    service::change_password(&state.db, id, input).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "users",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "User id")),
    responses(
        (status = 204, description = "User deleted"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission, self-delete, or last-superadmin"),
        (status = 404, description = "User not found"),
    )
)]
pub async fn delete_user(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<impl IntoResponse> {
    let authz = require_permission(&state, claims, Permission::UsersDelete).await?;
    let superadmin_role_id = state.superadmin_role_id;
    let actor_id = authz.id;
    state
        .db
        .transaction::<_, (), CoreError>(|tx| {
            Box::pin(async move {
                service::delete_user(tx, actor_id, id, superadmin_role_id).await
            })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_users, create_user))
        .routes(routes!(select_list))
        .routes(routes!(get_user, update_user, delete_user))
        .routes(routes!(change_password))
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(db) => AppError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core.into(),
    }
}
