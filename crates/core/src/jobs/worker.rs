//! Submission delivery worker.
//!
//! Long-running Tokio task that claims due `submission_delivery` rows from
//! MySQL (`SELECT … FOR UPDATE SKIP LOCKED`), dispatches them to the matching
//! [`Backend`](crate::backend::Backend), and records the outcome — with retry
//! and admin-notify fallback per the delivery-guarantee design.
//!
//! Skeleton implementation: the loop is wired up but no-ops until the
//! `submission_delivery` entity exists. The full claim/ack contract lands
//! with the Submissions resource.

use std::time::Duration;

use sea_orm::DatabaseConnection;
use tokio::time::sleep;
use tracing::{debug, info};

use crate::backend::registry::BackendRegistry;

#[derive(Clone)]
pub struct WorkerHandle {
    // kept for future shutdown / metrics hooks
    _marker: (),
}

/// Spawn the worker loop. Returns immediately with a handle.
pub fn spawn(db: DatabaseConnection, registry: BackendRegistry) -> WorkerHandle {
    tokio::spawn(run(db, registry));
    WorkerHandle { _marker: () }
}

async fn run(_db: DatabaseConnection, registry: BackendRegistry) {
    info!(
        backends = ?registry.names().collect::<Vec<_>>(),
        "submission delivery worker started"
    );

    // TODO(submissions): poll for due deliveries via
    //   SELECT ... FROM submission_delivery
    //   WHERE next_attempt_at <= NOW() AND status = 'pending'
    //   ORDER BY next_attempt_at LIMIT N FOR UPDATE SKIP LOCKED;
    // dispatch to registry, then mark sent / schedule retry / notify admins.
    loop {
        debug!("delivery worker tick (no-op until submission_delivery exists)");
        sleep(Duration::from_secs(30)).await;
    }
}
