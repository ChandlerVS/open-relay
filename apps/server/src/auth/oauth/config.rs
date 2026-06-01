//! OAuth provider config: public, admin, discovery.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use open_relay_core::oauth_config::service as oauth_config_service;
use open_relay_core::oauth_config::{
    DiscoveryPrefill, DiscoveryRequest, OAuthConfigDto, OAuthConfigPublicDto, UpsertOAuthConfig,
};
use open_relay_core::permissions::Permission;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::auth::permissions::require_permission;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/config",
    tag = "oauth",
    responses(
        (status = 200, description = "Public OAuth status", body = OAuthConfigPublicDto),
    )
)]
pub async fn public_config(State(state): State<AppState>) -> AppResult<Json<OAuthConfigPublicDto>> {
    let cfg = oauth_config_service::get_public(&state.db).await?;
    Ok(Json(cfg))
}

#[utoipa::path(
    get,
    path = "/admin-config",
    tag = "oauth",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Active OAuth config (no client_secret)", body = OAuthConfigDto),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "No active config"),
    )
)]
pub async fn admin_get_config(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<OAuthConfigDto>> {
    require_permission(&state, claims, Permission::AuthConfigWrite).await?;
    let model = oauth_config_service::get_active(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("no active oauth config".into()))?;
    Ok(Json(model.into()))
}

#[utoipa::path(
    post,
    path = "/admin-config",
    tag = "oauth",
    security(("bearer" = [])),
    request_body = UpsertOAuthConfig,
    responses(
        (status = 200, description = "OAuth config saved", body = OAuthConfigDto),
        (status = 400, description = "Validation failed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
    )
)]
pub async fn admin_upsert_config(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(input): Json<UpsertOAuthConfig>,
) -> AppResult<Json<OAuthConfigDto>> {
    require_permission(&state, claims, Permission::AuthConfigWrite).await?;
    let model = oauth_config_service::upsert(
        &state.db,
        &state.cipher,
        state.allow_private_network(),
        input,
    )
    .await?;
    Ok(Json(model.into()))
}

#[utoipa::path(
    delete,
    path = "/admin-config",
    tag = "oauth",
    security(("bearer" = [])),
    responses(
        (status = 204, description = "OAuth config removed"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 404, description = "No active config"),
    )
)]
pub async fn admin_delete_config(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<impl IntoResponse> {
    require_permission(&state, claims, Permission::AuthConfigWrite).await?;
    oauth_config_service::delete_active(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/discover",
    tag = "oauth",
    security(("bearer" = [])),
    request_body = DiscoveryRequest,
    responses(
        (status = 200, description = "Discovery doc parsed", body = DiscoveryPrefill),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Insufficient permission"),
        (status = 502, description = "Discovery endpoint failed"),
    )
)]
pub async fn admin_discover(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(input): Json<DiscoveryRequest>,
) -> AppResult<Json<DiscoveryPrefill>> {
    require_permission(&state, claims, Permission::AuthConfigWrite).await?;
    let prefill = oauth_config_service::discover(input, state.allow_private_network()).await?;
    Ok(Json(prefill))
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(public_config))
        .routes(routes!(
            admin_get_config,
            admin_upsert_config,
            admin_delete_config
        ))
        .routes(routes!(admin_discover))
}
