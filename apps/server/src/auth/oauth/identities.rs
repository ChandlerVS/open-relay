//! Current-user linked-identity endpoints.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use open_relay_core::external_identity::ExternalIdentityDto;
use open_relay_core::external_identity::service as external_identity_service;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::auth::permissions::authenticated_user;
use crate::error::AppResult;
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/identities",
    tag = "oauth",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Current user's linked identities", body = [ExternalIdentityDto]),
        (status = 401, description = "Missing or invalid token"),
    )
)]
pub async fn list_my_identities(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> AppResult<Json<Vec<ExternalIdentityDto>>> {
    let user = authenticated_user(claims)?;
    let rows = external_identity_service::list_for_user(&state.db, user.id).await?;
    Ok(Json(rows))
}

#[utoipa::path(
    delete,
    path = "/identities/{id}",
    tag = "oauth",
    security(("bearer" = [])),
    params(("id" = i32, Path, description = "Identity id")),
    responses(
        (status = 204, description = "Identity unlinked"),
        (status = 401, description = "Missing or invalid token"),
        (status = 403, description = "Would lock the user out"),
        (status = 404, description = "Identity not found"),
    )
)]
pub async fn delete_my_identity(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(identity_id): Path<i32>,
) -> AppResult<impl IntoResponse> {
    let user = authenticated_user(claims)?;
    external_identity_service::unlink(&state.db, user.id, identity_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_my_identities))
        .routes(routes!(delete_my_identity))
}
