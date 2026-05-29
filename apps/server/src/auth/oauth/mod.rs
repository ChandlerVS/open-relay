//! HTTP routes for the admin-configured OAuth provider.
//!
//! Sub-mounted under `/auth/oauth` by `crate::auth::router`.

pub mod config;
pub mod flow;
pub mod identities;

use utoipa_axum::router::OpenApiRouter;

use crate::state::AppState;

pub const STATE_COOKIE_NAME: &str = "oauth_state";
pub const STATE_COOKIE_PATH: &str = "/auth/oauth/";
pub const STATE_COOKIE_MAX_AGE_SECONDS: i64 = 600;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .merge(config::router())
        .merge(flow::router())
        .merge(identities::router())
}

/// Build a `Set-Cookie` value for the OAuth flow state envelope.
pub fn build_state_cookie(value: &str, secure: bool) -> String {
    build_cookie(STATE_COOKIE_NAME, value, STATE_COOKIE_MAX_AGE_SECONDS, secure)
}

/// Build a `Set-Cookie` value that expires the state cookie immediately.
pub fn build_state_cookie_clear(secure: bool) -> String {
    build_cookie(STATE_COOKIE_NAME, "", 0, secure)
}

fn build_cookie(name: &str, value: &str, max_age_seconds: i64, secure: bool) -> String {
    let mut parts = vec![
        format!("{}={}", name, value),
        format!("Path={}", STATE_COOKIE_PATH),
        "HttpOnly".to_string(),
        "SameSite=Lax".to_string(),
        format!("Max-Age={}", max_age_seconds),
    ];
    if secure {
        parts.push("Secure".to_string());
    }
    parts.join("; ")
}

/// Pull our state cookie out of the `Cookie:` header.
pub fn read_state_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let raw = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for entry in cookie::Cookie::split_parse(raw) {
        if let Ok(c) = entry {
            if c.name() == STATE_COOKIE_NAME {
                return Some(c.value().to_string());
            }
        }
    }
    None
}
