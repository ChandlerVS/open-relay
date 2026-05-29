//! Framework-agnostic error type for core services.
//!
//! HTTP frameworks adapt these variants to their own response types — the
//! server crate maps `CoreError` to `AppError` (and thus to HTTP statuses).
//! Core code never speaks HTTP directly.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("unauthorized")]
    Unauthorized,

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("internal error")]
    Internal(#[from] anyhow::Error),

    #[error("database error")]
    Db(#[from] sea_orm::DbErr),
}

pub type CoreResult<T> = Result<T, CoreError>;
