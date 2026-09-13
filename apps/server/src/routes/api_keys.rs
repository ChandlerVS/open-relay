//! Self-service API key management.
//!
//! Deliberately gated by `authenticated_user` rather than `require_permission`,
//! for the reason `crates/core/src/api_keys/mod.rs` sets out: a key's effective
//! permissions are always its owner's permissions intersected with its scopes,
//! so minting one grants nothing the caller did not already hold. Adding a
//! `api_keys:write` permission would only create a way to lock a user out of a
//! credential they can already exercise by logging in.
//!
//! Every handler scopes to the caller's own keys inside the service, so another
//! user's key id 404s rather than 403s — an id is not worth confirming.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use open_relay_core::api_keys::{ApiKeyDto, CreatedApiKey, NewApiKey, service};
use open_relay_core::error::CoreError;
use sea_orm::TransactionTrait;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::auth::permissions::authenticated_user;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "",
    tag = "api-keys",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "The caller's API keys", body = Vec<ApiKeyDto>),
        (status = 401, description = "Unauthenticated"),
    )
)]
pub async fn list_api_keys(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<Vec<ApiKeyDto>>> {
    let authz = authenticated_user(claims)?;
    let keys = service::list_for_user(&state.db, authz.id).await?;
    Ok(Json(keys))
}

#[utoipa::path(
    post,
    path = "",
    tag = "api-keys",
    security(("bearer" = [])),
    request_body = NewApiKey,
    responses(
        (status = 201, description = "Key created; `token` is returned once and never again", body = CreatedApiKey),
        (status = 400, description = "Invalid name, scopes or expiry"),
        (status = 401, description = "Unauthenticated"),
        (status = 403, description = "Scoped to a permission the caller does not hold"),
    )
)]
pub async fn create_api_key(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(input): Json<NewApiKey>,
) -> AppResult<impl IntoResponse> {
    let authz = authenticated_user(claims)?;
    let user_id = authz.id;
    let created = state
        .db
        .transaction::<_, CreatedApiKey, CoreError>(|tx| {
            Box::pin(async move { service::issue(tx, user_id, input).await })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok((StatusCode::CREATED, Json(created)))
}

#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "api-keys",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "API key id")),
    responses(
        (status = 204, description = "Revoked"),
        (status = 401, description = "Unauthenticated"),
        (status = 404, description = "No such active key belonging to the caller"),
    )
)]
pub async fn revoke_api_key(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i32>,
) -> AppResult<impl IntoResponse> {
    let authz = authenticated_user(claims)?;
    service::revoke(&state.db, authz.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(db) => AppError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core.into(),
    }
}

pub fn router() -> OpenApiRouter<AppState> {
    // The limiter covers the whole surface rather than just the POST. `routes!`
    // groups by path, so the list and the create share one entry and cannot be
    // layered apart — and rate-limiting reads of a credential list alongside
    // writes of one is the conservative reading anyway. The profile page loads
    // this once, so the login limiter's budget is ample.
    OpenApiRouter::new()
        .routes(routes!(list_api_keys, create_api_key))
        .routes(routes!(revoke_api_key))
        .layer(crate::ratelimit::login_layer())
}
