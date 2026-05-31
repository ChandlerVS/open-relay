use axum::Router;
use axum::http::HeaderValue;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::Method;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::SwaggerUi;

use crate::routes;
use crate::state::AppState;

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
    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest("/healthz", routes::health::router())
        .nest("/auth", crate::auth::router())
        .nest("/setup", routes::setup::router())
        .nest("/dashboard", routes::dashboard::router())
        .nest("/users", routes::users::router())
        .nest("/roles", routes::roles::router())
        .nest("/permissions", routes::roles::permissions_router())
        .nest("/forms", routes::forms::router())
        .nest("/backends", routes::backends::router())
        .nest("/submissions", routes::submissions::router())
        .nest("/public/forms", routes::public_forms::router())
        .split_for_parts();

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
        .merge(SwaggerUi::new("/docs").url("/openapi.json", api))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
