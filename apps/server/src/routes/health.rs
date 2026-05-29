use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::state::AppState;

#[derive(Serialize, ToSchema)]
pub struct Health {
    pub status: &'static str,
}

#[utoipa::path(
    get,
    path = "",
    tag = "health",
    responses((status = 200, description = "Service is up", body = Health))
)]
pub async fn get_health() -> Json<Health> {
    Json(Health { status: "ok" })
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_health))
}
