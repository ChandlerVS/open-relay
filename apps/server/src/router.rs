use axum::Router;
use tower_http::cors::CorsLayer;
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
        (name = "setup", description = "First-time bootstrap."),
    ),
)]
struct ApiDoc;

pub fn build(state: AppState) -> Router {
    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest("/healthz", routes::health::router())
        .nest("/auth", crate::auth::router())
        .nest("/setup", routes::setup::router())
        .split_for_parts();

    router
        .merge(SwaggerUi::new("/docs").url("/openapi.json", api))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
