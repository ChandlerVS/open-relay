use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::HeaderValue;
use axum::http::Method;
use axum::http::header::{
    AUTHORIZATION, CONTENT_SECURITY_POLICY, CONTENT_TYPE, REFERRER_POLICY,
    STRICT_TRANSPORT_SECURITY, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::SwaggerUi;

use crate::routes;
use crate::state::AppState;

/// Tight body cap for the unauthenticated public surface (form submissions are
/// small JSON). Pairs with rate limiting to blunt cheap DoS.
const PUBLIC_BODY_LIMIT: usize = 64 * 1024;
/// Roomier cap for the authenticated admin API (form schemas etc. stay small
/// but want headroom).
const ADMIN_BODY_LIMIT: usize = 1024 * 1024;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "OpenRelay API",
        description = "Form orchestration backend.",
    ),
    tags(
        (name = "health", description = "Liveness / readiness."),
        (name = "auth", description = "Local + SSO authentication."),
        (name = "oauth", description = "Admin-configured OAuth provider + account linking."),
        (name = "setup", description = "First-time bootstrap."),
        (name = "dashboard", description = "Aggregate admin overview."),
        (name = "users", description = "User management."),
        (name = "roles", description = "Roles + permission catalogue."),
        (name = "forms", description = "Form schemas embedded by host pages."),
        (name = "backends", description = "Configured delivery backends (e.g. GoHighLevel)."),
        (name = "submissions", description = "Form submissions and their per-backend delivery state."),
        (name = "public", description = "Unauthenticated endpoints consumed by embedded forms."),
    ),
)]
struct ApiDoc;

pub fn build(state: AppState) -> Router {
    let is_production = state.environment == crate::config::Environment::Production;

    // Public, unauthenticated surface (embedded forms + health). Gets a tight
    // body limit and non-frame-restricting headers — the embeddable form MUST
    // stay frameable on third-party host pages, so no X-Frame-Options here.
    let (public_router, public_api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest("/healthz", routes::health::router())
        .nest("/public/forms", routes::public_forms::router())
        .split_for_parts();
    let public_router = public_router
        .layer(SetResponseHeaderLayer::overriding(
            X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(DefaultBodyLimit::max(PUBLIC_BODY_LIMIT));

    // Authenticated admin/API surface. Frame-denied + the full header set.
    let (admin_router, admin_api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest("/auth", crate::auth::router())
        .nest("/setup", routes::setup::router())
        .nest("/dashboard", routes::dashboard::router())
        .nest("/users", routes::users::router())
        .nest("/roles", routes::roles::router())
        .nest("/permissions", routes::roles::permissions_router())
        .nest("/forms", routes::forms::router())
        .nest("/backends", routes::backends::router())
        .nest("/submissions", routes::submissions::router())
        .split_for_parts();
    let mut admin_router = admin_router
        .layer(SetResponseHeaderLayer::overriding(
            X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("frame-ancestors 'none'"),
        ))
        .layer(DefaultBodyLimit::max(ADMIN_BODY_LIMIT));
    if is_production {
        // HSTS assumes a TLS-terminating proxy in front (see deployment docs).
        admin_router = admin_router.layer(SetResponseHeaderLayer::overriding(
            STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=63072000; includeSubDomains"),
        ));
    }

    // Merge the two axum routers and union their OpenAPI specs.
    let mut router = admin_router.merge(public_router);
    let mut api = admin_api;
    api.merge(public_api);

    // Swagger UI + raw spec are dev-only: in production we don't publish the
    // full API surface. Kept on in development so `pnpm gen:api` can scrape it.
    if !is_production {
        router = router.merge(SwaggerUi::new("/docs").url("/openapi.json", api));
    }

    // CORS that supports credentials (needed for the OAuth state cookie to be
    // set on cross-origin POST responses). Origin allowlist comes from
    // `ADMIN_URL`; "*" is not valid with credentials.
    //
    // `ADMIN_URL` is validated as a header-safe origin at config load
    // (`Config::validate`), so this parse succeeds in practice. The `Err`
    // branch fails *closed* — a deny-all CORS layer (no allowed origin) rather
    // than `CorsLayer::permissive()`, so a bad origin can never widen access.
    let cors = match HeaderValue::from_str(&state.admin_url) {
        Ok(origin) => CorsLayer::new()
            .allow_origin(AllowOrigin::exact(origin))
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([AUTHORIZATION, CONTENT_TYPE])
            .allow_credentials(true),
        Err(_) => {
            tracing::error!(
                admin_url = %state.admin_url,
                "ADMIN_URL is not a valid CORS origin; denying all cross-origin requests"
            );
            CorsLayer::new()
        }
    };

    router
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
