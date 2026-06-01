use anyhow::Context;
use sea_orm::Database;
use tracing_subscriber::{EnvFilter, fmt};

mod auth;
mod config;
mod error;
mod jobs;
mod ratelimit;
mod router;
mod routes;
mod state;

use crate::config::Config;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("info,open_relay_server=debug,open_relay_core=debug")
        }))
        .with_target(true)
        .init();

    let config = Config::from_env().context("loading configuration")?;
    tracing::info!(listen = %config.listen_addr, "open-relay-server starting");

    let db = Database::connect(&config.database_url)
        .await
        .context("connecting to MySQL")?;

    // SeaORM 2.0 entity-first schema sync. The `entity::*` glob discovers every
    // module declared in `crates/entity/src/lib.rs` via the entity-registry
    // feature. Idempotent: creates missing tables/columns/keys, leaves the rest.
    db.get_schema_registry("entity::*")
        .sync(&db)
        .await
        .context("syncing entity schema to MySQL")?;
    tracing::info!("entity schema sync complete");

    let superadmin_role_id = open_relay_core::rbac::service::ensure_superadmin(&db)
        .await
        .context("seeding superadmin role")?;
    tracing::info!(superadmin_role_id, "rbac superadmin role synchronized");

    // One-shot: populate `form.backends` on rows created before the column
    // existed. Idempotent; subsequent boots affect zero rows.
    let backfilled = open_relay_core::forms::service::backfill_default_backends(&db)
        .await
        .context("backfilling default form backends")?;
    if backfilled > 0 {
        tracing::info!(backfilled, "applied default backends to legacy form rows");
    }

    let state = AppState::new(db.clone(), &config, superadmin_role_id)?;

    // Spawn delivery worker.
    jobs::spawn(db.clone(), state.backends.clone(), state.cipher.clone());

    let app = router::build(state.clone());

    let listener = tokio::net::TcpListener::bind(&config.listen_addr)
        .await
        .with_context(|| format!("binding {}", config.listen_addr))?;
    tracing::info!(listen = %config.listen_addr, "ready");
    // `into_make_service_with_connect_info` exposes the peer `SocketAddr` so the
    // per-IP rate limiters (tower_governor `PeerIpKeyExtractor`) can key on it.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .context("axum::serve")?;

    Ok(())
}
