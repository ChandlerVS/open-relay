//! Submission admin routes — list, export, get, delete, bulk delete.
//!
//! The public POST endpoint (the one the embed SDK actually targets) lives
//! in `routes/public_forms.rs` since it's nested under the form id and is
//! unauthenticated.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::response::IntoResponse;
use open_relay_core::error::CoreError;
use open_relay_core::permissions::Permission;
use open_relay_core::submissions::{
    BulkDeleteRequest, BulkDeleteResponse, ExportQuery, ListQuery, RetryDeliveriesRequest,
    RetryDeliveriesResponse, SubmissionDto, SubmissionList, service,
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

#[utoipa::path(
    post,
    path = "/deliveries/retry",
    tag = "submissions",
    security(("bearer" = [])),
    request_body = RetryDeliveriesRequest,
    responses(
        (status = 200, description = "Delivery rows re-queued (partitioned report)", body = RetryDeliveriesResponse),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn retry_deliveries(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<RetryDeliveriesRequest>,
) -> AppResult<Json<RetryDeliveriesResponse>> {
    require_permission(&state, claims, Permission::SubmissionsRetry).await?;
    let report = service::retry_deliveries(&state.db, &req).await?;
    Ok(Json(report))
}

#[utoipa::path(
    get,
    path = "/export",
    tag = "submissions",
    security(("bearer" = [])),
    params(ExportQuery),
    responses(
        (status = 200, description = "Matching submissions as CSV", content_type = "text/csv", body = String),
        (status = 400, description = "Invalid filter, or too many matching submissions"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn export_submissions(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Query(q): Query<ExportQuery>,
) -> AppResult<impl IntoResponse> {
    require_permission(&state, claims, Permission::SubmissionsRead).await?;
    let filter = q.filter()?;
    let csv = service::export_csv(&state.db, &filter).await?;
    let disposition = format!("attachment; filename=\"{}\"", service::export_filename());
    Ok((
        [
            (CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (CONTENT_DISPOSITION, disposition),
        ],
        csv,
    ))
}

#[utoipa::path(
    post,
    path = "/bulk-delete",
    tag = "submissions",
    security(("bearer" = [])),
    request_body = BulkDeleteRequest,
    responses(
        (status = 200, description = "Submissions deleted", body = BulkDeleteResponse),
        (status = 400, description = "Too many ids"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn bulk_delete_submissions(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<BulkDeleteRequest>,
) -> AppResult<Json<BulkDeleteResponse>> {
    require_permission(&state, claims, Permission::SubmissionsDelete).await?;
    let deleted = state
        .db
        .transaction::<_, u64, CoreError>(|tx| {
            Box::pin(async move { service::delete_submissions(tx, &req.ids).await })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok(Json(BulkDeleteResponse { deleted }))
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(db) => AppError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core.into(),
    }
}

pub fn router() -> OpenApiRouter<AppState> {
    // The static paths are registered ahead of `/{id}`; axum prefers a static
    // segment regardless, but keeping them first keeps the intent readable.
    OpenApiRouter::new()
        .routes(routes!(list_submissions))
        .routes(routes!(export_submissions))
        .routes(routes!(bulk_delete_submissions))
        .routes(routes!(retry_deliveries))
        .routes(routes!(get_submission, delete_submission))
}
