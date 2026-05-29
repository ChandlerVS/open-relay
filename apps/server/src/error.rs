use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("not implemented: {0}")]
    NotImplemented(&'static str),

    #[error("unauthorized")]
    Unauthorized,

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("internal error")]
    Internal(#[from] anyhow::Error),

    #[error("database error")]
    Db(#[from] sea_orm::DbErr),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotImplemented(_) => (StatusCode::NOT_IMPLEMENTED, self.to_string()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::Internal(err) => {
                tracing::error!(?err, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
            }
            AppError::Db(err) => {
                tracing::error!(?err, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
