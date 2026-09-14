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
use open_relay_core::storage::uploads::{self, UploadTicketDto, UploadTicketRequest};
use open_relay_core::storage_config;
use open_relay_core::submissions::service::UploadContext;
use open_relay_core::themes::service as themes_service;
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
    let theme_id = form.theme_id;
    let mut dto = forms_service::public_dto_from_model(form)?;
    dto.theme = themes_service::resolve_settings(&state.db, theme_id).await?;
    // Only pay for the storage lookup when the form actually has a file field
    // — the same stance `needs_subdivisions` takes about the region table.
    // Asked of the DTO's already-parsed layout, so the form JSON is read once.
    if forms_service::needs_uploads(&dto.layout) {
        dto.uploads_enabled = storage_config::service::is_configured(&state.db).await?;
    }
    Ok(Json(dto))
}

/// Mint a presigned upload for one `file` field.
///
/// Unauthenticated by necessity — embedded forms run on host pages we don't
/// own — so it carries its own, tighter rate limit and every control described
/// in `open_relay_core::storage::uploads`. Bytes never reach this server: the
/// browser PUTs them straight to the configured store.
#[utoipa::path(
    post,
    path = "/{id}/uploads",
    tag = "public",
    params(("id" = i32, Path, description = "Form id")),
    request_body = UploadTicketRequest,
    responses(
        (status = 200, description = "Presigned upload", body = UploadTicketDto),
        (status = 400, description = "Not a file field, too large, or wrong type"),
        (status = 404, description = "Form not found"),
        (status = 503, description = "No storage provider is configured"),
    )
)]
pub async fn create_upload_ticket(
    State(state): State<AppState>,
    Path(form_id): Path<i32>,
    Json(req): Json<UploadTicketRequest>,
) -> AppResult<Json<UploadTicketDto>> {
    let form = forms_service::find_by_id(&state.db, form_id)
        .await?
        .ok_or_else(|| AppError::NotFound("form not found".into()))?;
    let layout = forms_service::layout_from_model(&form)?;

    let store = storage_config::service::load_active_store(&state.db, &state.storage, &state.cipher)
        .await?
        .ok_or_else(|| {
            // An operator problem, not a visitor one: the form asks for a file
            // but nothing was configured to hold it.
            AppError::ServiceUnavailable("file uploads are not configured".into())
        })?;

    let ticket = uploads::issue_ticket(&state.cipher, &store, form.id, &layout, &req).await?;
    Ok(Json(ticket))
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
    // Owned clones: the transaction closure outlives this borrow of `state`,
    // and both are cheap (an `Arc` and a `HashMap` of `Arc`s).
    let cipher = state.cipher.clone();
    let storage = state.storage.clone();
    let id = state
        .db
        .transaction::<_, i32, CoreError>(|tx| {
            Box::pin(async move {
                let uploads = UploadContext {
                    cipher: &cipher,
                    registry: &storage,
                };
                let form = forms_service::find_by_id(tx, form_id)
                    .await?
                    .ok_or_else(|| CoreError::NotFound("form not found".into()))?;
                let inserted =
                    submissions_service::create_submission(tx, &form, payload, &uploads).await?;
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
        // A tighter, dedicated bucket for presigning. This route is the only
        // unauthenticated path that causes a write into the operator's object
        // store, so it must not share its allowance with schema GETs.
        .merge(
            OpenApiRouter::new()
                .routes(routes!(create_upload_ticket))
                .layer(crate::ratelimit::upload_layer()),
        )
        .layer(crate::ratelimit::public_layer())
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(db) => AppError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core.into(),
    }
}
