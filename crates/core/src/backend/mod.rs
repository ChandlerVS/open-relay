//! Delivery backends: the pluggable surface that takes a submission and
//! relays it onward (OpenRelay's own store, GoHighLevel, Salesforce, …).
//!
//! Each integration implements [`Backend`] and registers itself in a
//! [`registry::BackendRegistry`] at startup.

pub mod openrelay;
pub mod registry;

use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;

pub use openrelay::OpenRelayBackend;
pub use registry::BackendRegistry;

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
