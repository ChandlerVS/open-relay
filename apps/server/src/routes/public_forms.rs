//! Public, unauthenticated form schema endpoint.
//!
//! Consumed by the embed SDK / `@open-relay/form-renderer` running in
//! third-party host pages. Returns just the renderer needs — no owner id,
//! no audit timestamps — via [`PublicFormDto`].

use axum::Json;
use axum::extract::{Path, State};
use open_relay_core::forms::{PublicFormDto, service};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/{id}",
    tag = "public",
    params(("id" = i32, Path, description = "Form id")),
    responses(
        (status = 200, description = "Public form schema", body = PublicFormDto),
        (status = 404, description = "Form not found"),
    )
)]
pub async fn get_public_form(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> AppResult<Json<PublicFormDto>> {
    let form = service::find_by_id(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("form not found".into()))?;
    let dto = service::public_dto_from_model(form)?;
    Ok(Json(dto))
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_public_form))
}
