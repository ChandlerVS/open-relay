//! Delivery backends: the pluggable surface that takes a submission and
//! relays it onward (OpenRelay's own store, GoHighLevel, Salesforce, …).
//!
//! Two flavours of registration:
//!
//! - **Static singletons** — backends that need no per-instance config
//!   (today: only `open-relay`). Registered once at boot via
//!   [`BackendRegistry::register_static`] and shared via `Arc<dyn Backend>`.
//! - **Configurable factories** — backends whose credentials/parameters
//!   come from a `backend_instance` DB row (today: `gohighlevel`).
//!   Registered via [`BackendRegistry::register_factory`]; the worker
//!   instantiates a fresh `Backend` per delivery by feeding the row's
//!   `config` JSON to [`BackendFactory::build`].

pub mod gohighlevel;
pub mod openrelay;
pub mod registry;

use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;

pub use gohighlevel::{GoHighLevelBackend, GoHighLevelConfig, GoHighLevelFactory};
pub use openrelay::OpenRelayBackend;
pub use registry::{BackendKindInfo, BackendRegistry};

/// A pending submission handed to a backend for delivery.
#[derive(Debug, Clone)]
pub struct DeliveryPayload {
    pub submission_id: i32,
    pub form_id: i32,
    /// Merged view of the submission: standard field columns + custom_data,
    /// keyed by field key. Backends should consume `data` rather than rely on
    /// any particular column layout.
    pub data: Value,
}

#[derive(Debug, Error)]
pub enum DeliveryError {
    /// Transient failure — the worker should retry per the configured policy.
    #[error("transient delivery failure: {0}")]
    Transient(String),
    /// Permanent failure — do not retry, notify admins.
    #[error("permanent delivery failure: {0}")]
    Permanent(String),
}

#[async_trait]
pub trait Backend: Send + Sync + 'static {
    /// Stable identifier used to look this backend up from configuration.
    fn name(&self) -> &'static str;

    /// Deliver a single submission. Implementations should be idempotent on
    /// `submission_id` — the worker may re-invoke after a crash mid-delivery.
    async fn deliver(&self, payload: &DeliveryPayload) -> Result<(), DeliveryError>;
}

/// Returned by a factory when the stored config can't be parsed/validated.
/// Reaches the worker as a permanent delivery failure (config can't fix
/// itself without admin intervention).
#[derive(Debug, Error)]
pub enum BackendBuildError {
    #[error("invalid backend config: {0}")]
    Invalid(String),
}

/// Builds a configured `Backend` from a stored `backend_instance` row.
pub trait BackendFactory: Send + Sync + 'static {
    /// Stable kind identifier shared with the `backend_instance.kind` column.
    fn kind(&self) -> &'static str;

    /// Top-level keys in this kind's `config` JSON that hold secrets (API
    /// tokens, refresh tokens, …). These are redacted from admin-facing DTOs
    /// and preserved across partial updates that omit them. Default: none.
    fn secret_keys(&self) -> &'static [&'static str] {
        &[]
    }

    /// Parse + validate the row's `config` JSON and yield a ready-to-call
    /// backend. Called every delivery — implementations should keep this
    /// cheap (clone a shared `reqwest::Client`, etc.) rather than spin up
    /// new connection pools per call.
    fn build(&self, config: &Value) -> Result<Arc<dyn Backend>, BackendBuildError>;
}
