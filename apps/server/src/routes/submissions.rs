//! Submission admin routes — list, get, delete.
//!
//! The public POST endpoint (the one the embed SDK actually targets) lives
//! in `routes/public_forms.rs` since it's nested under the form id and is
//! unauthenticated.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use open_relay_core::permissions::Permission;
use open_relay_core::submissions::{ListQuery, SubmissionDto, SubmissionList, service};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::auth::permissions::require_permission;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "",
    tag = "submissions",
    security(("bearer" = [])),
    params(ListQuery),
    responses(
        (status = 200, description = "Paginated submissions", body = SubmissionList),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn list_submissions(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<SubmissionList>> {
    require_permission(&state, claims, Permission::SubmissionsRead).await?;
    let list = service::list(&state.db, &q).await?;
    Ok(Json(list))
}

#[utoipa::path(
    get,
    path = "/{id}",
    tag = "submissions",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Submission id")),
    responses(
        (status = 200, description = "Submission with delivery rows", body = SubmissionDto),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Submission not found"),
    )
)]
pub async fn get_submission(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<Json<SubmissionDto>> {
    require_permission(&state, claims, Permission::SubmissionsRead).await?;
    let dto = service::dto_for_id(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("submission not found".into()))?;
    Ok(Json(dto))
}

#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "submissions",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Submission id")),
    responses(
        (status = 204, description = "Submission deleted"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Submission not found"),
    )
)]
pub async fn delete_submission(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<impl IntoResponse> {
    require_permission(&state, claims, Permission::SubmissionsDelete).await?;
    service::delete_submission(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_submissions))
        .routes(routes!(get_submission, delete_submission))
}
