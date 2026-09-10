//! Admin CRUD for the object-storage provider that backs `file` form fields.
//!
//! Singleton, like the OAuth provider config it mirrors: one active row, and
//! `storage_config:write` gates reads too — the only read *is* the admin form,
//! and it reports which deployment credentials are on record.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use open_relay_core::error::CoreError;
use open_relay_core::permissions::Permission;
use open_relay_core::storage::StorageKindInfo;
use open_relay_core::storage_config::{
    StorageConfigDto, StorageTestResult, UpsertStorageConfig, service as storage_service,
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
    tag = "storage",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Active storage provider", body = StorageConfigDto),
        (status = 401, description = "Unauthenticated"),
        (status = 403, description = "Missing storage_config:write"),
        (status = 404, description = "No storage provider configured"),
    )
)]
pub async fn get_storage_config(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<StorageConfigDto>> {
    require_permission(&state, claims, Permission::StorageConfigWrite).await?;
    let model = storage_service::get_active(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("no storage provider configured".into()))?;
    Ok(Json(storage_service::to_dto(&state.storage, model)))
}

#[utoipa::path(
    get,
    path = "/kinds",
    tag = "storage",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Supported storage kinds", body = Vec<StorageKindInfo>),
        (status = 401, description = "Unauthenticated"),
        (status = 403, description = "Missing storage_config:write"),
    )
)]
pub async fn list_storage_kinds(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<Vec<StorageKindInfo>>> {
    require_permission(&state, claims, Permission::StorageConfigWrite).await?;
    Ok(Json(state.storage.kinds()))
}

#[utoipa::path(
    post,
    path = "",
    tag = "storage",
    security(("bearer" = [])),
    request_body = UpsertStorageConfig,
    responses(
        (status = 200, description = "Storage provider saved", body = StorageConfigDto),
        (status = 400, description = "Config rejected by the provider"),
        (status = 401, description = "Unauthenticated"),
        (status = 403, description = "Missing storage_config:write"),
    )
)]
pub async fn upsert_storage_config(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(input): Json<UpsertStorageConfig>,
) -> AppResult<Json<StorageConfigDto>> {
    require_permission(&state, claims, Permission::StorageConfigWrite).await?;
    let registry = state.storage.clone();
    let cipher = state.cipher.clone();
    let model = state
        .db
        .transaction::<_, entity::storage_provider::Model, CoreError>(|tx| {
            Box::pin(
                async move { storage_service::upsert(tx, &registry, &cipher, input).await },
            )
        })
        .await
        .map_err(unwrap_tx)?;
    Ok(Json(storage_service::to_dto(&state.storage, model)))
}

#[utoipa::path(
    delete,
    path = "",
    tag = "storage",
    security(("bearer" = [])),
    responses(
        (status = 204, description = "Storage provider removed"),
        (status = 401, description = "Unauthenticated"),
        (status = 403, description = "Missing storage_config:write"),
        (status = 404, description = "No storage provider configured"),
    )
)]
pub async fn delete_storage_config(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<impl IntoResponse> {
    require_permission(&state, claims, Permission::StorageConfigWrite).await?;
    storage_service::delete_active(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Probe the saved credentials by round-tripping a real object.
///
/// Always 200 when a provider is configured — a reachable-but-rejecting store
/// is the *answer* to this request, carried in `StorageTestResult::error`, not
/// an error in serving it. That keeps the admin UI's failure copy on one path.
#[utoipa::path(
    post,
    path = "/test",
    tag = "storage",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Probe result", body = StorageTestResult),
        (status = 401, description = "Unauthenticated"),
        (status = 403, description = "Missing storage_config:write"),
        (status = 404, description = "No storage provider configured"),
    )
)]
pub async fn test_storage_config(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<StorageTestResult>> {
    require_permission(&state, claims, Permission::StorageConfigWrite).await?;
    Ok(Json(
        storage_service::test_active(&state.db, &state.storage, &state.cipher).await?,
    ))
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(
            get_storage_config,
            upsert_storage_config,
            delete_storage_config
        ))
        .routes(routes!(list_storage_kinds))
        .routes(routes!(test_storage_config))
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(db) => AppError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core.into(),
    }
}
