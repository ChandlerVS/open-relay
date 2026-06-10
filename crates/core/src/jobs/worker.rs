//! Submission delivery worker.
//!
//! Long-running Tokio task that leases due `submission_delivery` rows from
//! MySQL (`SELECT … FOR UPDATE SKIP LOCKED`), dispatches them to the matching
//! [`Backend`](crate::backend::Backend), and records the outcome.
//!
//! Lifecycle of a delivery row, driven by this loop:
//!
//! ```text
//! pending ─lease──▶ in_progress ─Ok──────▶ succeeded
//!    ▲                   │
//!    │                   ├── Transient ──▶ pending  (next_attempt_at = now + backoff)
//!    │                                     OR exhausted (attempts >= MAX_ATTEMPTS)
//!    │                   └── Permanent ─▶ permanent_failure
//!    │
//!    └── stale-lease sweep on startup re-queues anything stuck in_progress.
//! ```

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ConnectionTrait, DatabaseConnection, DatabaseTransaction,
    EntityTrait, Statement, TransactionTrait,
};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use crate::backend::{Backend, BackendBuildError, DeliveryError, DeliveryPayload};
use crate::backend::registry::BackendRegistry;
use crate::crypto::SecretCipher;
use crate::submissions::service::{
    STATUS_EXHAUSTED, STATUS_IN_PROGRESS, STATUS_PENDING, STATUS_PERMANENT_FAILURE,
    STATUS_SUCCEEDED, delivery_data,
};
use crate::forms::service::tags_from_model;

const IDLE_INTERVAL: Duration = Duration::from_secs(5);
const BATCH_SIZE: u32 = 16;
const MAX_ATTEMPTS: i32 = 6;
/// How long a leased row is allowed to stay `in_progress` before the startup
/// sweep treats it as orphaned (e.g. the worker crashed mid-delivery).
const STALE_LEASE: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub struct WorkerHandle {
    _marker: (),
}

pub fn spawn(
    db: DatabaseConnection,
    registry: BackendRegistry,
    cipher: Arc<SecretCipher>,
) -> WorkerHandle {
    tokio::spawn(run(db, registry, cipher));
    WorkerHandle { _marker: () }
}

async fn run(db: DatabaseConnection, registry: BackendRegistry, cipher: Arc<SecretCipher>) {
    let kinds: Vec<String> = registry.kinds().into_iter().map(|k| k.kind).collect();
    info!(backends = ?kinds, "submission delivery worker started");

    if let Err(e) = reclaim_stale_leases(&db).await {
        warn!(error = ?e, "stale-lease reclaim failed at startup");
    }

    loop {
        match lease_and_deliver_batch(&db, &registry, &cipher).await {
            Ok(n) if n > 0 => continue,
            Ok(_) => sleep(IDLE_INTERVAL).await,
            Err(e) => {
                warn!(error = ?e, "delivery batch failed");
                sleep(IDLE_INTERVAL).await;
            }
        }
    }
}

/// Re-queue anything stuck in `in_progress` past the lease deadline. Idempotent.
async fn reclaim_stale_leases(db: &DatabaseConnection) -> anyhow::Result<()> {
    let cutoff = Utc::now() - chrono::Duration::from_std(STALE_LEASE)?;
    let stmt = Statement::from_sql_and_values(
        db.get_database_backend(),
        "UPDATE submission_delivery
         SET status = ?, updated_at = ?
         WHERE status = ? AND updated_at < ?",
        [
            STATUS_PENDING.into(),
            Utc::now().into(),
            STATUS_IN_PROGRESS.into(),
            cutoff.into(),
        ],
    );
    let res = db.execute_raw(stmt).await?;
    if res.rows_affected() > 0 {
        info!(reclaimed = res.rows_affected(), "reclaimed stale delivery leases");
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct Leased {
    id: i32,
    submission_id: i32,
    backend_name: String,
    backend_instance_id: Option<i32>,
    attempts: i32,
}

/// Returns the number of rows attempted in this batch (0 means the queue is idle).
async fn lease_and_deliver_batch(
    db: &DatabaseConnection,
    registry: &BackendRegistry,
    cipher: &SecretCipher,
) -> anyhow::Result<usize> {
    let leased = lease_batch(db).await?;
    if leased.is_empty() {
        return Ok(0);
    }
    let count = leased.len();
    for row in leased {
        if let Err(e) = dispatch_one(db, registry, cipher, row).await {
            // Already logged inside dispatch_one; soak so the batch keeps moving.
            error!(error = ?e, "dispatch returned error after handling");
        }
    }
    Ok(count)
}

async fn lease_batch(db: &DatabaseConnection) -> anyhow::Result<Vec<Leased>> {
    let tx = db.begin().await?;
    let backend = tx.get_database_backend();
    let pick = Statement::from_sql_and_values(
        backend,
        "SELECT id, submission_id, backend_name, backend_instance_id, attempts
         FROM submission_delivery
         WHERE status = ? AND next_attempt_at <= ?
         ORDER BY next_attempt_at ASC
         LIMIT ?
         FOR UPDATE SKIP LOCKED",
        [
            STATUS_PENDING.into(),
            Utc::now().into(),
            (BATCH_SIZE as i32).into(),
        ],
    );
    let rows = tx.query_all_raw(pick).await?;
    if rows.is_empty() {
        tx.commit().await?;
        return Ok(Vec::new());
    }

    let mut leased: Vec<Leased> = Vec::with_capacity(rows.len());
    for r in &rows {
        leased.push(Leased {
            id: r.try_get::<i32>("", "id")?,
            submission_id: r.try_get::<i32>("", "submission_id")?,
            backend_name: r.try_get::<String>("", "backend_name")?,
            backend_instance_id: r.try_get::<Option<i32>>("", "backend_instance_id")?,
            attempts: r.try_get::<i32>("", "attempts")?,
        });
    }

    let ids: Vec<i32> = leased.iter().map(|r| r.id).collect();
    let now = Utc::now();
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "UPDATE submission_delivery
         SET status = ?, attempts = attempts + 1, updated_at = ?
         WHERE id IN ({placeholders})"
    );
    let mut values: Vec<sea_orm::Value> = Vec::with_capacity(ids.len() + 2);
    values.push(STATUS_IN_PROGRESS.into());
    values.push(now.into());
    for id in &ids {
        values.push((*id).into());
    }
    tx.execute_raw(Statement::from_sql_and_values(backend, sql, values))
        .await?;

    tx.commit().await?;

    // Bump the in-memory `attempts` to match what we just persisted so the
    // retry-budget check uses the new value.
    for row in &mut leased {
        row.attempts += 1;
    }
    Ok(leased)
}

async fn dispatch_one(
    db: &DatabaseConnection,
    registry: &BackendRegistry,
    cipher: &SecretCipher,
    row: Leased,
) -> anyhow::Result<()> {
    let submission = entity::submission::Entity::find_by_id(row.submission_id)
        .one(db)
        .await?;
    let submission = match submission {
        Some(s) => s,
        None => {
            warn!(
                delivery_id = row.id,
                submission_id = row.submission_id,
                "orphaned delivery row — marking permanent_failure"
            );
            mark_permanent(db, row.id, "submission row missing").await?;
            return Ok(());
        }
    };

    let tags = match entity::form::Entity::find_by_id(submission.form_id)
        .one(db)
        .await?
    {
        Some(form) => match tags_from_model(&form) {
            Ok(t) => t,
            Err(e) => {
                warn!(
                    delivery_id = row.id,
                    form_id = submission.form_id,
                    error = ?e,
                    "form tags parse failed — continuing with empty tags"
                );
                Vec::new()
            }
        },
        None => {
            warn!(
                delivery_id = row.id,
                form_id = submission.form_id,
                "form row missing — continuing with empty tags"
            );
            Vec::new()
        }
    };

    let backend: Arc<dyn Backend> = match row.backend_instance_id {
        None => match registry.get_static(&row.backend_name) {
            Some(b) => b,
            None => {
                warn!(
                    delivery_id = row.id,
                    backend = %row.backend_name,
                    "static backend not registered — marking permanent_failure"
                );
                mark_permanent(db, row.id, "backend not registered").await?;
                return Ok(());
            }
        },
        Some(instance_id) => {
            let inst = entity::backend_instance::Entity::find_by_id(instance_id)
                .one(db)
                .await?;
            let inst = match inst {
                Some(i) => i,
                None => {
                    warn!(
                        delivery_id = row.id,
                        backend = %row.backend_name,
                        instance_id,
                        "backend instance row missing — marking permanent_failure"
                    );
                    mark_permanent(db, row.id, "backend instance row missing").await?;
                    return Ok(());
                }
            };
            if inst.kind != row.backend_name {
                warn!(
                    delivery_id = row.id,
                    expected = %row.backend_name,
                    actual = %inst.kind,
                    "backend instance kind mismatch — marking permanent_failure"
                );
                mark_permanent(db, row.id, "backend instance kind mismatch").await?;
                return Ok(());
            }
            let factory = match registry.get_factory(&inst.kind) {
                Some(f) => f,
                None => {
                    warn!(
                        delivery_id = row.id,
                        backend = %inst.kind,
                        "backend factory not registered — marking permanent_failure"
                    );
                    mark_permanent(db, row.id, "backend factory not registered").await?;
                    return Ok(());
                }
            };
            // Secret keys are encrypted at rest; decrypt them into a plaintext
            // copy of the config just before handing it to the factory.
            let mut config = inst.config.clone();
            if let Err(e) =
                crate::backends::service::decrypt_secret_keys(registry, &inst.kind, &mut config, cipher)
            {
                warn!(
                    delivery_id = row.id,
                    backend = %inst.kind,
                    instance_id,
                    error = ?e,
                    "backend config secret decrypt failed — marking permanent_failure"
                );
                mark_permanent(db, row.id, "backend config decrypt failed").await?;
                return Ok(());
            }
            match factory.build(&config) {
                Ok(b) => b,
                Err(BackendBuildError::Invalid(msg)) => {
                    warn!(
                        delivery_id = row.id,
                        backend = %inst.kind,
                        instance_id,
                        error = %msg,
                        "backend config invalid — marking permanent_failure"
                    );
                    mark_permanent(db, row.id, &format!("invalid backend config: {msg}")).await?;
                    return Ok(());
                }
            }
        }
    };

    let payload = DeliveryPayload {
        submission_id: submission.id,
        form_id: submission.form_id,
        data: delivery_data(&submission),
        tags,
    };

    match backend.deliver(&payload).await {
        Ok(()) => {
            debug!(
                delivery_id = row.id,
                backend = %row.backend_name,
                "delivery succeeded"
            );
            mark_succeeded(db, row.id).await?;
        }
        Err(DeliveryError::Permanent(msg)) => {
            warn!(
                delivery_id = row.id,
                backend = %row.backend_name,
                error = %msg,
                "permanent delivery failure"
            );
            mark_permanent(db, row.id, &msg).await?;
        }
        Err(DeliveryError::Transient(msg)) => {
            if row.attempts >= MAX_ATTEMPTS {
                warn!(
                    delivery_id = row.id,
                    attempts = row.attempts,
                    error = %msg,
                    "delivery exhausted retries"
                );
                mark_exhausted(db, row.id, &msg).await?;
            } else {
                let when = next_attempt_after(row.attempts);
                debug!(
                    delivery_id = row.id,
                    attempts = row.attempts,
                    retry_at = %when,
                    "transient failure — scheduling retry"
                );
                mark_pending_retry(db, row.id, when, &msg).await?;
            }
        }
    }
    Ok(())
}

/// Exponential backoff schedule. `attempts` is the number of attempts already
/// made (post-increment in `lease_batch`), so this maps:
/// 1 → 30s, 2 → 2m, 3 → 10m, 4 → 1h, 5 → 6h, 6+ → 24h.
fn next_attempt_after(attempts: i32) -> DateTime<Utc> {
    let delay = match attempts {
        0 | 1 => Duration::from_secs(30),
        2 => Duration::from_secs(2 * 60),
        3 => Duration::from_secs(10 * 60),
        4 => Duration::from_secs(60 * 60),
        5 => Duration::from_secs(6 * 60 * 60),
        _ => Duration::from_secs(24 * 60 * 60),
    };
    Utc::now() + chrono::Duration::from_std(delay).expect("delay in range")
}

async fn mark_succeeded(db: &DatabaseConnection, id: i32) -> anyhow::Result<()> {
    update_status_with(db, id, |m| {
        m.status = ActiveValue::Set(STATUS_SUCCEEDED.into());
        m.delivered_at = ActiveValue::Set(Some(Utc::now()));
        m.last_error = ActiveValue::Set(None);
    })
    .await
}

async fn mark_permanent(db: &DatabaseConnection, id: i32, msg: &str) -> anyhow::Result<()> {
    let msg = msg.to_string();
    update_status_with(db, id, |m| {
        m.status = ActiveValue::Set(STATUS_PERMANENT_FAILURE.into());
        m.last_error = ActiveValue::Set(Some(msg));
    })
    .await
}

async fn mark_exhausted(db: &DatabaseConnection, id: i32, msg: &str) -> anyhow::Result<()> {
    let msg = msg.to_string();
    update_status_with(db, id, |m| {
        m.status = ActiveValue::Set(STATUS_EXHAUSTED.into());
        m.last_error = ActiveValue::Set(Some(msg));
    })
    .await
}

async fn mark_pending_retry(
    db: &DatabaseConnection,
    id: i32,
    when: DateTime<Utc>,
    msg: &str,
) -> anyhow::Result<()> {
    let msg = msg.to_string();
    update_status_with(db, id, |m| {
        m.status = ActiveValue::Set(STATUS_PENDING.into());
        m.next_attempt_at = ActiveValue::Set(when);
        m.last_error = ActiveValue::Set(Some(msg));
    })
    .await
}

async fn update_status_with<F>(db: &DatabaseConnection, id: i32, mutate: F) -> anyhow::Result<()>
where
    F: FnOnce(&mut entity::submission_delivery::ActiveModel),
{
    let existing = entity::submission_delivery::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("delivery row vanished before status update"))?;
    let mut active: entity::submission_delivery::ActiveModel = existing.into();
    mutate(&mut active);
    active.update(db).await?;
    Ok(())
}

#[allow(dead_code)]
fn _typecheck_transaction(_tx: &DatabaseTransaction) {
    // Compile-time check that `DatabaseTransaction` implements ConnectionTrait,
    // useful when refactoring the helpers above to take a generic conn.
    fn assert_conn<C: ConnectionTrait>(_: &C) {}
    let _ = assert_conn::<DatabaseTransaction>;
}
