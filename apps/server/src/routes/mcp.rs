//! The MCP surface: `POST /api/v1/mcp`, streamable HTTP.
//!
//! Mounted as a third surface alongside the admin and public routers, and
//! deliberately **outside** the `OpenApiRouter`. MCP is JSON-RPC over a single
//! path — there is nothing for OpenAPI to describe, and adding it to the spec
//! would put a bogus operation into the generated TypeScript client.
//!
//! # Authentication
//!
//! [`mcp_auth`] runs before rmcp sees anything and 401s on the spot if the
//! credential does not resolve, so an unauthenticated caller never opens a
//! session. On success it puts an [`ApiActor`] into the request extensions.
//!
//! Getting that actor to a tool handler is the one piece of this integration
//! that rmcp's own examples do not demonstrate. rmcp forwards the inbound
//! request into `RequestContext::extensions` as a whole `http::request::Parts`
//! — not as individual extension types — so the handler reads it back out one
//! level down. `OpenRelayMcp::actor` is the other half of this contract; the
//! two have to change together.
//!
//! Note rmcp only attaches those parts on **POST**. That covers `initialize`
//! and every `tools/call`; the GET (SSE) and DELETE (session teardown) paths
//! carry no actor, which is why authentication is enforced in the middleware
//! rather than left to the tools.

use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use open_relay_core::api_keys::{TOKEN_PREFIX, service as api_keys};
use open_relay_core::auth::verify_jwt;
use open_relay_core::rbac::service as rbac_service;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};

use crate::error::AppError;
use crate::state::AppState;

/// Resolve the bearer credential to an actor, or reject.
///
/// Two credential shapes are accepted. An API key is the one an agent will
/// use — long-lived, revocable, and the reason this endpoint is usable at all.
/// A normal access JWT is also accepted so an operator already logged into the
/// admin UI can point a tool at the endpoint without minting anything; it
/// simply expires in fifteen minutes like everywhere else.
async fn mcp_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let credential = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)?
        .to_string();

    let actor = if credential.starts_with(TOKEN_PREFIX) {
        api_keys::authenticate(&state.db, &credential).await?
    } else {
        let claims = verify_jwt(&state.auth_keys, &credential).map_err(AppError::from)?;
        let user_id: i32 = claims.sub.parse().map_err(|_| AppError::Unauthorized)?;
        let permissions = rbac_service::load_user_permissions(&state.db, user_id).await?;
        open_relay_core::api_keys::ApiActor {
            user_id,
            permissions,
        }
    };

    request.extensions_mut().insert(actor);
    Ok(next.run(request).await)
}

/// Hosts this deployment answers to, for rmcp's `Host` header check.
///
/// The check defaults to loopback only, which would reject every request to a
/// deployed server — a DNS-rebinding guard that fails closed. Rather than add
/// another environment variable, the allowlist is derived from the URLs the
/// deployment already declares, with the loopback names kept so local
/// development keeps working unchanged.
fn allowed_hosts(public_api_url: &str, admin_url: &str) -> Vec<String> {
    let mut hosts = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];
    for url in [public_api_url, admin_url] {
        // A bare authority is enough; rmcp compares against the `Host` header,
        // which carries host[:port] and no scheme or path.
        let Some(authority) = url
            .split("://")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .filter(|a| !a.is_empty())
        else {
            continue;
        };
        if !hosts.iter().any(|h| h == authority) {
            hosts.push(authority.to_string());
        }
        // `Host` omits the default port, so a configured `:443`/`:80` has to be
        // matched by its bare form too.
        if let Some((host, port)) = authority.rsplit_once(':') {
            if (port == "443" || port == "80") && !hosts.iter().any(|h| h == host) {
                hosts.push(host.to_string());
            }
        }
    }
    hosts
}

pub fn router(state: AppState) -> Router<AppState> {
    let mcp_state = state.clone();
    let config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(allowed_hosts(&state.public_api_url, &state.admin_url))
        // Match the admin surface's body limit: a 300-element layout is the
        // largest thing that legitimately crosses this boundary.
        .with_max_request_body_bytes(crate::router::ADMIN_BODY_LIMIT);

    let service = StreamableHttpService::new(
        move || {
            Ok(open_relay_mcp::handler::OpenRelayMcp::http(
                mcp_state.db.clone(),
                mcp_state.backends.clone(),
            ))
        },
        LocalSessionManager::default().into(),
        config,
    );

    Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(state, mcp_auth))
        .layer(crate::ratelimit::mcp_layer())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deployed_host_is_allowed_alongside_loopback() {
        let hosts = allowed_hosts("https://api.example.com", "https://admin.example.com");
        assert!(hosts.contains(&"api.example.com".to_string()));
        assert!(hosts.contains(&"admin.example.com".to_string()));
        // Local development must keep working with no configuration.
        assert!(hosts.contains(&"localhost".to_string()));
        assert!(hosts.contains(&"127.0.0.1".to_string()));
    }

    #[test]
    fn an_explicit_default_port_also_matches_its_bare_form() {
        // A `Host` header omits :443, so a config that spells it out must still
        // match the header an agent actually sends.
        let hosts = allowed_hosts("https://api.example.com:443", "http://admin.example.com:80");
        assert!(hosts.contains(&"api.example.com".to_string()));
        assert!(hosts.contains(&"admin.example.com".to_string()));
    }

    #[test]
    fn a_nondefault_port_is_kept_verbatim() {
        let hosts = allowed_hosts("http://localhost:8080", "http://localhost:5173");
        assert!(hosts.contains(&"localhost:8080".to_string()));
        assert!(hosts.contains(&"localhost:5173".to_string()));
    }

    /// The list must never come back empty: rmcp treats an empty `allowed_hosts`
    /// as "allow any Host", so a malformed URL silently disabling the
    /// DNS-rebinding guard would be the worst possible failure mode.
    #[test]
    fn a_malformed_url_still_leaves_the_guard_armed() {
        let hosts = allowed_hosts("not-a-url", "");
        assert!(hosts.contains(&"localhost".to_string()));
        assert!(!hosts.is_empty());
    }

    #[test]
    fn duplicate_urls_are_not_repeated() {
        let hosts = allowed_hosts("http://localhost:8080", "http://localhost:8080");
        assert_eq!(
            hosts.iter().filter(|h| *h == "localhost:8080").count(),
            1
        );
    }
}
