//! Admin CRUD for the outbound-email provider that sends submission
//! notifications.
//!
//! Singleton, like the storage provider it mirrors: one active row, and
//! `email_config:write` gates reads too — the only read *is* the admin form,
//! and it reports which deployment credentials are on record.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use open_relay_core::email::EmailKindInfo;
use open_relay_core::email_config::{
    EmailConfigDto, EmailTestRequest, EmailTestResult, UpsertEmailConfig, service as email_service,
};
use open_relay_core::error::CoreError;
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
    tag = "email",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Active email provider", body = EmailConfigDto),
        (status = 401, description = "Unauthenticated"),
        (status = 403, description = "Missing email_config:write"),
        (status = 404, description = "No email provider configured"),
    )
)]
pub async fn get_email_config(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<EmailConfigDto>> {
    require_permission(&state, claims, Permission::EmailConfigWrite).await?;
    let model = email_service::get_active(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("no email provider configured".into()))?;
    Ok(Json(email_service::to_dto(&state.email, model)))
}

#[utoipa::path(
    get,
    path = "/kinds",
    tag = "email",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Supported email provider kinds", body = Vec<EmailKindInfo>),
        (status = 401, description = "Unauthenticated"),
        (status = 403, description = "Missing email_config:write"),
    )
)]
pub async fn list_email_kinds(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<Vec<EmailKindInfo>>> {
    require_permission(&state, claims, Permission::EmailConfigWrite).await?;
    Ok(Json(state.email.kinds()))
}

#[utoipa::path(
    post,
    path = "",
    tag = "email",
    security(("bearer" = [])),
    request_body = UpsertEmailConfig,
    responses(
        (status = 200, description = "Email provider saved", body = EmailConfigDto),
        (status = 400, description = "Config rejected by the provider"),
        (status = 401, description = "Unauthenticated"),
        (status = 403, description = "Missing email_config:write"),
    )
)]
pub async fn upsert_email_config(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(input): Json<UpsertEmailConfig>,
) -> AppResult<Json<EmailConfigDto>> {
    require_permission(&state, claims, Permission::EmailConfigWrite).await?;
    let registry = state.email.clone();
    let cipher = state.cipher.clone();
    let model = state
        .db
        .transaction::<_, entity::email_provider::Model, CoreError>(|tx| {
            Box::pin(async move { email_service::upsert(tx, &registry, &cipher, input).await })
        })
        .await
        .map_err(unwrap_tx)?;
    Ok(Json(email_service::to_dto(&state.email, model)))
}

#[utoipa::path(
    delete,
    path = "",
    tag = "email",
    security(("bearer" = [])),
    responses(
        (status = 204, description = "Email provider removed"),
        (status = 401, description = "Unauthenticated"),
        (status = 403, description = "Missing email_config:write"),
        (status = 404, description = "No email provider configured"),
    )
)]
pub async fn delete_email_config(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<impl IntoResponse> {
    require_permission(&state, claims, Permission::EmailConfigWrite).await?;
    email_service::delete_active(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Send a real test message through the saved provider.
///
/// Always 200 when a provider is configured — a rejection (typically an
/// unverified recipient in the SES sandbox) is the *answer* to this request,
/// carried in `EmailTestResult::error`.
#[utoipa::path(
    post,
    path = "/test",
    tag = "email",
    security(("bearer" = [])),
    request_body = EmailTestRequest,
    responses(
        (status = 200, description = "Send result", body = EmailTestResult),
        (status = 400, description = "Recipient is not a valid address"),
        (status = 401, description = "Unauthenticated"),
        (status = 403, description = "Missing email_config:write"),
        (status = 404, description = "No email provider configured"),
    )
)]
pub async fn test_email_config(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(input): Json<EmailTestRequest>,
) -> AppResult<Json<EmailTestResult>> {
    require_permission(&state, claims, Permission::EmailConfigWrite).await?;
    Ok(Json(
        email_service::send_test(&state.db, &state.email, &state.cipher, &input.to).await?,
    ))
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(
            get_email_config,
            upsert_email_config,
            delete_email_config
        ))
        .routes(routes!(list_email_kinds))
        .routes(routes!(test_email_config))
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(db) => AppError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core.into(),
    }
}
