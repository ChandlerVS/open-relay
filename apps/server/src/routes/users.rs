//! User management routes — CRUD plus a lightweight `select-list` for
//! dropdowns.
//!
//! All endpoints require an authenticated bearer token. There is no role
//! gating at this layer; RBAC will land separately.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use open_relay_core::error::CoreError;
use open_relay_core::users::{
    ChangePassword, ListQuery, NewUser, UpdateUser, UserDto, UserList, UserSelectOption, service,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
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
    )
)]
pub async fn list_users(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<UserList>> {
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
    )
)]
pub async fn select_list(
    State(state): State<AppState>,
    _user: AuthUser,
) -> AppResult<Json<Vec<UserSelectOption>>> {
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
        (status = 404, description = "User not found"),
    )
)]
pub async fn get_user(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<i32>,
) -> AppResult<Json<UserDto>> {
    let user = service::find_by_id(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".into()))?;
    Ok(Json(user.into()))
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
        (status = 409, description = "Email already in use"),
    )
)]
pub async fn create_user(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(input): Json<NewUser>,
) -> AppResult<impl IntoResponse> {
    let created = service::create_user(&state.db, input).await?;
    Ok((StatusCode::CREATED, Json(UserDto::from(created))))
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
        (status = 404, description = "User not found"),
        (status = 409, description = "Email already in use"),
    )
)]
pub async fn update_user(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<i32>,
    Json(input): Json<UpdateUser>,
) -> AppResult<Json<UserDto>> {
    let updated = service::update_user(&state.db, id, input).await?;
    Ok(Json(updated.into()))
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
        (status = 404, description = "User not found"),
    )
)]
pub async fn change_password(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<i32>,
    Json(input): Json<ChangePassword>,
) -> AppResult<impl IntoResponse> {
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
        (status = 404, description = "User not found"),
        (status = 409, description = "Cannot delete the currently authenticated user"),
    )
)]
pub async fn delete_user(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<impl IntoResponse> {
    let current: i32 = claims
        .sub
        .parse()
        .map_err(|_| AppError::Unauthorized)?;
    if id == current {
        return Err(AppError::from(CoreError::Conflict(
            "cannot delete the currently authenticated user".into(),
        )));
    }
    service::delete_user(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_users, create_user))
        .routes(routes!(select_list))
        .routes(routes!(get_user, update_user, delete_user))
        .routes(routes!(change_password))
}
