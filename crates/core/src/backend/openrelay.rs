//! OpenRelay's own backend.
//!
//! Submissions are already persisted in the `submission` table at POST time
//! (that table is the canonical audit log + delivery queue). The OpenRelay
//! backend exists so "visible in the admin dashboard" is a deliberate
//! destination on equal footing with GoHighLevel etc. — selecting it on a
//! form just queues a delivery row whose success is the side-effect of the
//! submission already being stored.

use async_trait::async_trait;

use super::{Backend, DeliveryError, DeliveryPayload};

pub const NAME: &str = "open-relay";

pub struct OpenRelayBackend;

#[async_trait]
impl Backend for OpenRelayBackend {
    fn name(&self) -> &'static str {
        NAME
    }

    async fn deliver(&self, _payload: &DeliveryPayload) -> Result<(), DeliveryError> {
        Ok(())
    }
}
