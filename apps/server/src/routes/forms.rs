//! Form management routes — CRUD plus a lightweight `select-list`.
//!
//! Every endpoint is gated by a code-defined permission via
//! `require_permission`. The public read endpoint lives in
//! `routes/public_forms.rs` so its unauthenticated nature is obvious from
//! the file layout.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use open_relay_core::error::CoreError;
use open_relay_core::forms::{
    EmbedSnippetDto, FormDto, FormList, FormSelectOption, ListQuery, NewForm, UpdateForm, service,
};
use open_relay_core::permissions::Permission;
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
    tag = "forms",
    security(("bearer" = [])),
    params(ListQuery),
    responses(
        (status = 200, description = "Paginated forms", body = FormList),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn list_forms(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<FormList>> {
    require_permission(&state, claims, Permission::FormsRead).await?;
    let list = service::list_forms(&state.db, &q).await?;
    Ok(Json(list))
}

#[utoipa::path(
    get,
    path = "/select-list",
    tag = "forms",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Dropdown-friendly form list", body = [FormSelectOption]),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn form_select_list(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<Vec<FormSelectOption>>> {
    require_permission(&state, claims, Permission::FormsRead).await?;
    let rows = service::select_list(&state.db).await?;
    Ok(Json(rows))
}

#[utoipa::path(
    get,
    path = "/{id}",
    tag = "forms",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Form id")),
    responses(
        (status = 200, description = "Form", body = FormDto),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Form not found"),
    )
)]
pub async fn get_form(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<Json<FormDto>> {
    require_permission(&state, claims, Permission::FormsRead).await?;
    let form = service::find_by_id(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("form not found".into()))?;
    let dto = service::dto_from_model(&state.db, form).await?;
    Ok(Json(dto))
}

#[utoipa::path(
    get,
    path = "/{id}/embed",
    tag = "forms",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Form id")),
    responses(
        (status = 200, description = "Copy-paste embed snippet", body = EmbedSnippetDto),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Form not found"),
    )
)]
pub async fn get_embed_snippet(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<Json<EmbedSnippetDto>> {
    require_permission(&state, claims, Permission::FormsRead).await?;
    // Confirm the form exists (404 otherwise) so we never hand out a snippet
    // for a form that can't be embedded.
    let form = service::find_by_id(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("form not found".into()))?;
    // `data-api-url` is the base the embedded form-renderer fetches its schema
    // and posts submissions against (it appends `/public/forms/…`). The public
    // API lives under `/api/v1`, so hand the renderer that versioned base.
    let api_base = format!("{}/api/v1", state.public_api_url);
    let dto = EmbedSnippetDto::build(form.id, &state.embed_sdk_url, &api_base);
    Ok(Json(dto))
}

#[utoipa::path(
    post,
    path = "",
    tag = "forms",
    security(("bearer" = [])),
    request_body = NewForm,
    responses(
        (status = 201, description = "Form created", body = FormDto),
        (status = 400, description = "Validation failed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 409, description = "Slug already in use"),
    )
)]
pub async fn create_form(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(input): Json<NewForm>,
) -> AppResult<impl IntoResponse> {
    let authz = require_permission(&state, claims, Permission::FormsWrite).await?;
    let owner_id = authz.id;
    let registry = state.backends.clone();
    let dto = state
        .db
        .transaction::<_, FormDto, CoreError>(|tx| {
            Box::pin(async move {
                let created = service::create_form(tx, &registry, owner_id, input).await?;
                service::dto_from_model(tx, created).await
            })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok((StatusCode::CREATED, Json(dto)))
}

#[utoipa::path(
    patch,
    path = "/{id}",
    tag = "forms",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Form id")),
    request_body = UpdateForm,
    responses(
        (status = 200, description = "Form updated", body = FormDto),
        (status = 400, description = "Validation failed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Form not found"),
        (status = 409, description = "Slug already in use"),
    )
)]
pub async fn update_form(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
    Json(input): Json<UpdateForm>,
) -> AppResult<Json<FormDto>> {
    require_permission(&state, claims, Permission::FormsWrite).await?;
    let registry = state.backends.clone();
    let dto = state
        .db
        .transaction::<_, FormDto, CoreError>(|tx| {
            Box::pin(async move {
                let updated = service::update_form(tx, &registry, id, input).await?;
                service::dto_from_model(tx, updated).await
            })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok(Json(dto))
}

#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "forms",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Form id")),
    responses(
        (status = 204, description = "Form deleted"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "Form not found"),
    )
)]
pub async fn delete_form(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<impl IntoResponse> {
    require_permission(&state, claims, Permission::FormsDelete).await?;
    state
        .db
        .transaction::<_, (), CoreError>(|tx| {
            Box::pin(async move { service::delete_form(tx, id).await })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_forms, create_form))
        .routes(routes!(form_select_list))
        .routes(routes!(get_embed_snippet))
        .routes(routes!(get_form, update_form, delete_form))
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(db) => AppError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core.into(),
    }
}
