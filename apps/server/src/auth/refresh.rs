//! `/auth/refresh` and `/auth/logout` — the refresh-token half of the session.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use open_relay_core::auth::{self, RefreshRequest, TokenPair};
use open_relay_core::error::CoreError;
use sea_orm::TransactionTrait;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[utoipa::path(
    post,
    path = "/refresh",
    tag = "auth",
    request_body = RefreshRequest,
    responses(
        (status = 200, description = "Rotated token pair", body = TokenPair),
        (status = 401, description = "Invalid, expired, or revoked refresh token"),
    )
)]
pub async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> AppResult<Json<TokenPair>> {
    // No `AuthUser` gate: the access token is expected to be expired here. The
    // refresh secret itself is the credential. Rotation runs in a transaction
    // so revoke-old + issue-new are atomic.
    let (user, refresh_token) = state
        .db
        .transaction::<_, (entity::user::Model, String), CoreError>(|tx| {
            Box::pin(async move { auth::refresh::rotate(tx, &req.refresh_token).await })
        })
        .await
        .map_err(unwrap_tx)?;
    let token = auth::issue_for_user(&state.auth_keys, &user)?;
    Ok(Json(TokenPair {
        token,
        refresh_token,
    }))
}

#[utoipa::path(
    post,
    path = "/logout",
    tag = "auth",
    security(("bearer" = [])),
    responses(
        (status = 204, description = "Session revoked"),
        (status = 401, description = "Missing or invalid token"),
    )
)]
pub async fn logout(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<impl IntoResponse> {
    let user_id: i32 = claims.sub.parse().map_err(|_| AppError::Unauthorized)?;
    auth::refresh::revoke_all_for_user(&state.db, user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Collapse SeaORM's `TransactionError` wrapper into our `AppError`.
fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(db) => AppError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core.into(),
    }
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(refresh))
        .routes(routes!(logout))
}
