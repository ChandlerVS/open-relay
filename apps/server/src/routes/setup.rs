//! First-time bootstrap routes.
//!
//! `POST /setup/initialize` creates the very first user. It's "one-shot":
//! after any user exists in the DB it returns 409. The check + insert run
//! in a single transaction with `lock_exclusive` so concurrent calls can't
//! both succeed.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use open_relay_core::auth;
use open_relay_core::error::CoreError;
use open_relay_core::setup::{InitializeResponse, SetupStatus};
use open_relay_core::users::NewUser;
use open_relay_core::users::service;
use sea_orm::{EntityTrait, PaginatorTrait, QuerySelect, TransactionTrait};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[utoipa::path(
    post,
    path = "/initialize",
    tag = "setup",
    request_body = NewUser,
    responses(
        (status = 201, description = "Initial admin user created", body = InitializeResponse),
        (status = 400, description = "Validation failed"),
        (status = 409, description = "Already initialized"),
    )
)]
pub async fn initialize(
    State(state): State<AppState>,
    Json(input): Json<NewUser>,
) -> AppResult<impl IntoResponse> {
    let user = state
        .db
        .transaction::<_, entity::user::Model, CoreError>(|tx| {
            Box::pin(async move {
                let existing = entity::user::Entity::find()
                    .lock_exclusive()
                    .one(tx)
                    .await?;
                if existing.is_some() {
                    return Err(CoreError::Conflict("already initialized".into()));
                }
                service::create_user(tx, input).await
            })
        })
        .await
        .map_err(unwrap_tx_error)?;

    tracing::warn!(
        user_id = user.id,
        email = %user.email,
        "initial admin user bootstrapped"
    );

    let token = auth::issue_for_user(&state.auth_keys, &user)?;
    let body = InitializeResponse {
        token,
        user: user.into(),
    };
    Ok((StatusCode::CREATED, Json(body)))
}

#[utoipa::path(
    get,
    path = "/status",
    tag = "setup",
    responses((status = 200, description = "Setup state", body = SetupStatus))
)]
pub async fn status(State(state): State<AppState>) -> AppResult<Json<SetupStatus>> {
    let count = entity::user::Entity::find().count(&state.db).await?;
    Ok(Json(SetupStatus {
        initialized: count > 0,
    }))
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(initialize, status))
}

/// SeaORM wraps user transaction errors in `TransactionError::Transaction(e)`
/// and connection errors in `TransactionError::Connection(db_err)`. Collapse
/// to `AppError` via the core → app mapping.
fn unwrap_tx_error(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(db) => AppError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core.into(),
    }
}
