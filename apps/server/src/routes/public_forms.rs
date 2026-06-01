//! Public, unauthenticated form endpoints.
//!
//! Consumed by the embed SDK / `@open-relay/form-renderer` running in
//! third-party host pages. Returns just what the renderer needs — no owner
//! id, no audit timestamps — via [`PublicFormDto`]. The submission POST
//! handler accepts a form fill-out and queues delivery to every backend
//! bound to the form.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use open_relay_core::error::CoreError;
use open_relay_core::forms::{PublicFormDto, service as forms_service};
use open_relay_core::submissions::{
    NewSubmissionPayload, SubmissionAcceptedDto, service as submissions_service,
};
use sea_orm::TransactionTrait;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/{id}",
    tag = "public",
    params(("id" = i32, Path, description = "Form id")),
    responses(
        (status = 200, description = "Public form schema", body = PublicFormDto),
        (status = 404, description = "Form not found"),
    )
)]
pub async fn get_public_form(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> AppResult<Json<PublicFormDto>> {
    let form = forms_service::find_by_id(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("form not found".into()))?;
    let dto = forms_service::public_dto_from_model(form)?;
    Ok(Json(dto))
}

#[utoipa::path(
    post,
    path = "/{id}/submissions",
    tag = "public",
    params(("id" = i32, Path, description = "Form id")),
    request_body = NewSubmissionPayload,
    responses(
        (status = 201, description = "Submission accepted", body = SubmissionAcceptedDto),
        (status = 400, description = "Validation failed"),
        (status = 404, description = "Form not found"),
    )
)]
pub async fn create_submission_for_form(
    State(state): State<AppState>,
    Path(form_id): Path<i32>,
    Json(payload): Json<NewSubmissionPayload>,
) -> AppResult<impl IntoResponse> {
    let id = state
        .db
        .transaction::<_, i32, CoreError>(|tx| {
            Box::pin(async move {
                let form = forms_service::find_by_id(tx, form_id)
                    .await?
                    .ok_or_else(|| CoreError::NotFound("form not found".into()))?;
                let inserted = submissions_service::create_submission(tx, &form, payload).await?;
                Ok(inserted.id)
            })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok((StatusCode::CREATED, Json(SubmissionAcceptedDto { id })))
}

pub fn router() -> OpenApiRouter<AppState> {
    // Per-IP rate limit on the public form surface (schema GET + submission
    // POST) — blunts DB flooding and amplified CRM spam.
    OpenApiRouter::new()
        .routes(routes!(get_public_form, create_submission_for_form))
        .layer(crate::ratelimit::public_layer())
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(db) => AppError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core.into(),
    }
}
