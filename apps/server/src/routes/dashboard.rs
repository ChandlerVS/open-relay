//! Dashboard route — a single read-only aggregate for the admin landing page.
//!
//! Requires only authentication: every admin lands here. The recent-
//! submissions feed exposes individual submission content, so it is gated —
//! the handler does a soft `submissions:read` check and tells the core layer
//! whether to include it. Aggregate counts are returned to any authenticated
//! caller.

use axum::Json;
use axum::extract::State;
use open_relay_core::dashboard::{DashboardOverview, service};
use open_relay_core::permissions::Permission;
use open_relay_core::rbac::service as rbac_service;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "",
    tag = "dashboard",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Aggregate admin overview", body = DashboardOverview),
        (status = 401, description = "Missing or invalid token"),
    )
)]
pub async fn get_overview(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<DashboardOverview>> {
    let user_id: i32 = claims.sub.parse().map_err(|_| AppError::Unauthorized)?;
    let perms = rbac_service::load_user_permissions(&state.db, user_id).await?;
    let include_recent = perms.contains(&Permission::SubmissionsRead);
    let overview = service::overview(&state.db, include_recent).await?;
    Ok(Json(overview))
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_overview))
}
