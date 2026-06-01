use axum::Router;
use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::http::header::{
    AUTHORIZATION, CONTENT_SECURITY_POLICY, CONTENT_TYPE, REFERRER_POLICY,
    STRICT_TRANSPORT_SECURITY, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
};
use axum::http::{HeaderValue, Method, Request, StatusCode};
use axum::response::IntoResponse;
use tower::{ServiceBuilder, ServiceExt, service_fn};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
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
    // The whole API is served under this prefix (see `build`), so paths in this
    // spec are relative to it. Keeping it as a server base — rather than baking
    // `/api/v1` into every path — lets the generated TS client stay prefix-free
    // and carry the version in its `baseUrl`.
    servers((url = "/api/v1", description = "Versioned API root")),
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
    // Serve the built embed SDK bundle so a pasted `<script src=…/embed/open-relay.js>`
    // resolves on the same origin out of the box. `ServeFile` infers the
    // `text/javascript` content type from the extension.
    let public_router = public_router
        .route_service(
            "/embed/open-relay.js",
            ServeFile::new(&state.embed_sdk_path),
        )
        .layer(SetResponseHeaderLayer::overriding(
            X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(DefaultBodyLimit::max(PUBLIC_BODY_LIMIT))
        // Embedded forms run on arbitrary third-party origins: they fetch their
        // schema (GET) and post submissions (POST) cross-origin, and load the
        // SDK via a <script> tag. Allow any origin. No credentials — these
        // endpoints are unauthenticated and cookie-free, and `*` + credentials
        // is illegal anyway.
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::any())
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers([CONTENT_TYPE]),
        );

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

    // CORS for the admin surface: credentialed (the OAuth state cookie must be
    // set on cross-origin POST responses) and locked to `ADMIN_URL`; "*" is not
    // valid with credentials. This is deliberately stricter than the public
    // surface's allow-any policy above.
    //
    // `ADMIN_URL` is validated as a header-safe origin at config load
    // (`Config::validate`), so this parse succeeds in practice. The `Err`
    // branch fails *closed* — a deny-all CORS layer (no allowed origin) rather
    // than `CorsLayer::permissive()`, so a bad origin can never widen access.
    let admin_cors = match HeaderValue::from_str(&state.admin_url) {
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
    let admin_router = admin_router.layer(admin_cors);

    // Merge the two axum routers and union their OpenAPI specs. CORS is already
    // baked into each surface, so the merged router carries both policies.
    //
    // The whole JSON API (admin + public surfaces, plus `/healthz` and the embed
    // bundle) is then nested under a single `/api/v1` prefix. This keeps the API
    // off the root namespace so it never shadows the admin SPA's client-side
    // routes: a browser navigation to an SPA path like `/forms` or `/users` no
    // longer matches a same-named API handler (returning a 401 JSON), and instead
    // falls through to the catch-all SPA fallback below. Each surface's CORS,
    // body-limit and header layers were applied pre-merge, so they survive the
    // nest unchanged.
    let api_router = admin_router.merge(public_router);
    let mut router = Router::new().nest("/api/v1", api_router);
    let mut api = admin_api;
    api.merge(public_api);

    // Swagger UI + raw spec are dev-only: in production we don't publish the
    // full API surface. Kept on in development so `pnpm gen:api` can scrape it.
    if !is_production {
        router = router.merge(SwaggerUi::new("/docs").url("/openapi.json", api));
    }

    // Optionally serve the built admin SPA as the catch-all fallback. Every API
    // route is registered above and matches first; any other path falls through
    // here. Real files (hashed `/assets/*`, favicon, …) are served by `ServeDir`
    // as-is; anything else is a client-side route (hit on hard-refresh or a deep
    // link) and gets the SPA shell with a *200* so React Router can take over —
    // unlike `ServeDir::not_found_service`, which would return the shell under a
    // 404. Enabled only when `ADMIN_DIST_PATH` is set (the all-in-one image); in
    // local dev Vite serves the SPA on :5173. The shell carries the same
    // frame-denial + nosniff headers as the authenticated API surface, since it
    // must never be embeddable.
    if let Some(dir) = state.admin_dist_path.clone() {
        let index = format!("{dir}/index.html");
        let spa_fallback = service_fn(move |req: Request<Body>| {
            let dir = dir.clone();
            let index = index.clone();
            async move {
                let res = ServeDir::new(&dir).oneshot(req).await.into_response();
                if res.status() != StatusCode::NOT_FOUND {
                    return Ok::<_, std::convert::Infallible>(res);
                }
                // Unknown path → serve the SPA shell with 200 (client routing).
                let shell = match tokio::fs::read(&index).await {
                    Ok(html) => (
                        [(CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"))],
                        html,
                    )
                        .into_response(),
                    Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                };
                Ok(shell)
            }
        });
        let spa = ServiceBuilder::new()
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
            .service(spa_fallback);
        router = router.fallback_service(spa);
    }

    router.layer(TraceLayer::new_for_http()).with_state(state)
}
