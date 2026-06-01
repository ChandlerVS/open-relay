//! OAuth start + callback handlers.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use open_relay_core::auth;
use open_relay_core::auth::provider::Provider;
use open_relay_core::error::CoreError;
use open_relay_core::external_identity::service as external_identity_service;
use open_relay_core::oauth::oidc::OidcProvider;
use open_relay_core::oauth::state as oauth_state;
use open_relay_core::oauth::state::{OAuthFlowState, OAuthMode};
use open_relay_core::oauth_config::service as oauth_config_service;
use open_relay_core::users::service as users_service;
use sea_orm::TransactionTrait;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::{build_state_cookie, build_state_cookie_clear, read_state_cookie};
use crate::auth::AuthUser;
use crate::auth::permissions::authenticated_user;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const CALLBACK_PATH: &str = "/auth/oauth/callback";

#[derive(Debug, Serialize, ToSchema)]
pub struct LinkStartResponse {
    pub authorize_url: String,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// Build the authorize URL + state cookie for a given mode. Shared by both
/// the `GET /start` (anonymous, top-level redirect) and `POST /link/start`
/// (authenticated, JSON response) endpoints.
async fn build_authorize(
    state: &AppState,
    mode: OAuthMode,
) -> AppResult<(String, String)> {
    let cfg = oauth_config_service::get_active(&state.db)
        .await?
        .ok_or_else(|| AppError::from(CoreError::OAuthNotConfigured))?;

    let redirect_uri = format!("{}{}", state.public_api_url, CALLBACK_PATH);
    let provider = OidcProvider::from_config(&cfg, &state.cipher, state.allow_private_network())
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e.to_string())))?;
    let state_nonce = oauth_state::random_nonce();
    let oidc_nonce = oauth_state::random_nonce();
    let (authorize_url, pkce_verifier) = provider
        .authorize_with_pkce(&redirect_uri, &state_nonce, &oidc_nonce)
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e.to_string())))?;

    let mut flow = OAuthFlowState::new(mode, pkce_verifier);
    flow.nonce = state_nonce;
    flow.oidc_nonce = oidc_nonce;
    let cookie = oauth_state::issue_state(&state.auth_keys, &flow).map_err(AppError::from)?;
    Ok((authorize_url, cookie))
}

#[utoipa::path(
    get,
    path = "/start",
    tag = "oauth",
    responses(
        (status = 302, description = "Redirect to provider authorize endpoint"),
        (status = 404, description = "OAuth not configured"),
    )
)]
pub async fn start(State(state): State<AppState>) -> AppResult<Response> {
    let (authorize_url, cookie) = build_authorize(&state, OAuthMode::SignIn).await?;
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(
        axum::http::header::LOCATION,
        HeaderValue::from_str(&authorize_url)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("location header: {e}")))?,
    );
    resp_headers.insert(
        axum::http::header::SET_COOKIE,
        HeaderValue::from_str(&build_state_cookie(&cookie, state.cookie_secure))
            .map_err(|e| AppError::Internal(anyhow::anyhow!("set-cookie: {e}")))?,
    );
    Ok((StatusCode::FOUND, resp_headers).into_response())
}

#[utoipa::path(
    post,
    path = "/link/start",
    tag = "oauth",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Authorize URL + state cookie set", body = LinkStartResponse),
        (status = 401, description = "Missing or invalid token"),
        (status = 404, description = "OAuth not configured"),
    )
)]
pub async fn link_start(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Response> {
    let user = authenticated_user(claims)?;
    let (authorize_url, cookie) =
        build_authorize(&state, OAuthMode::Link { user_id: user.id }).await?;
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(
        axum::http::header::SET_COOKIE,
        HeaderValue::from_str(&build_state_cookie(&cookie, state.cookie_secure))
            .map_err(|e| AppError::Internal(anyhow::anyhow!("set-cookie: {e}")))?,
    );
    Ok((
        StatusCode::OK,
        resp_headers,
        Json(LinkStartResponse { authorize_url }),
    )
        .into_response())
}

#[utoipa::path(
    get,
    path = "/callback",
    tag = "oauth",
    params(CallbackQuery),
    responses(
        (status = 303, description = "Redirect to admin SPA with token (sign-in) or status (link)"),
        (status = 400, description = "Missing/invalid state or code"),
        (status = 404, description = "OAuth not configured"),
        (status = 502, description = "Token exchange failed"),
    )
)]
pub async fn callback(
    State(state): State<AppState>,
    Query(q): Query<CallbackQuery>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let clear_cookie = build_state_cookie_clear(state.cookie_secure);

    // Provider returned an error to us — surface it to the admin SPA.
    if let Some(err) = q.error.as_deref() {
        let msg = q
            .error_description
            .as_deref()
            .unwrap_or(err)
            .to_string();
        return Ok(redirect_with_error(&state.admin_url, &msg, &clear_cookie));
    }

    let code = q
        .code
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("missing code".into()))?;
    let state_param = q
        .state
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("missing state".into()))?;

    let cookie_value =
        read_state_cookie(&headers).ok_or_else(|| AppError::from(CoreError::OAuthStateMismatch))?;
    let flow = oauth_state::verify_state(&state.auth_keys, &cookie_value, state_param)
        .map_err(AppError::from)?;

    let cfg = oauth_config_service::get_active(&state.db)
        .await?
        .ok_or_else(|| AppError::from(CoreError::OAuthNotConfigured))?;
    let provider = OidcProvider::from_config(&cfg, &state.cipher, state.allow_private_network())
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e.to_string())))?;
    let redirect_uri = format!("{}{}", state.public_api_url, CALLBACK_PATH);
    let verified = provider
        .exchange(code, &redirect_uri, Some(&flow.pkce_verifier), &flow.oidc_nonce)
        .await
        .map_err(|e| AppError::from(CoreError::OAuthExchangeFailed(e.to_string())))?;

    match flow.mode {
        OAuthMode::SignIn => {
            let (token, refresh_token) = state
                .db
                .transaction::<_, (String, String), CoreError>(|tx| {
                    let auth_keys = state.auth_keys.clone();
                    let superadmin_role_id = state.superadmin_role_id;
                    let default_role_id = cfg.default_role_id;
                    let provider_id = cfg.id;
                    let verified = verified.clone();
                    Box::pin(async move {
                        let user = users_service::find_or_create_via_oauth(
                            tx,
                            provider_id,
                            &verified,
                            default_role_id,
                            superadmin_role_id,
                        )
                        .await?;
                        let token = auth::issue_for_user(&auth_keys, &user)?;
                        let refresh_token = auth::refresh::issue(tx, user.id).await?;
                        Ok((token, refresh_token))
                    })
                })
                .await
                .map_err(unwrap_tx)?;
            Ok(redirect_with_signin(
                &state.admin_url,
                &token,
                &refresh_token,
                &clear_cookie,
            ))
        }
        OAuthMode::Link { user_id } => {
            external_identity_service::link_to_user(
                &state.db,
                user_id,
                cfg.id,
                &verified.subject,
                verified.email.as_deref(),
            )
            .await?;
            Ok(redirect_with_link(&state.admin_url, &clear_cookie))
        }
    }
}

fn redirect_with_signin(
    admin_url: &str,
    token: &str,
    refresh_token: &str,
    clear_cookie: &str,
) -> Response {
    // Both tokens ride the URL fragment (never sent to the server, unlike a
    // query string), where the SPA reads and immediately clears them.
    let location = format!(
        "{}/oauth/callback#token={}&refresh={}&mode=signin",
        admin_url, token, refresh_token
    );
    redirect_303(&location, clear_cookie)
}

fn redirect_with_link(admin_url: &str, clear_cookie: &str) -> Response {
    let location = format!("{}/oauth/callback?mode=link&status=ok", admin_url);
    redirect_303(&location, clear_cookie)
}

fn redirect_with_error(admin_url: &str, message: &str, clear_cookie: &str) -> Response {
    let location = format!(
        "{}/oauth/callback?error={}",
        admin_url,
        urlencode(message)
    );
    redirect_303(&location, clear_cookie)
}

fn redirect_303(location: &str, clear_cookie: &str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::LOCATION,
        HeaderValue::from_str(location).unwrap_or_else(|_| HeaderValue::from_static("/")),
    );
    if let Ok(v) = HeaderValue::from_str(clear_cookie) {
        headers.insert(axum::http::header::SET_COOKIE, v);
    }
    (StatusCode::SEE_OTHER, headers).into_response()
}

fn urlencode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> AppError {
    match err {
        sea_orm::TransactionError::Connection(e) => AppError::Db(e),
        sea_orm::TransactionError::Transaction(e) => AppError::from(e),
    }
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(start))
        .routes(routes!(link_start))
        .routes(routes!(callback))
}
