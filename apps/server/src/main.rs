use anyhow::Context;
use sea_orm::Database;
use tracing_subscriber::{EnvFilter, fmt};

mod auth;
mod config;
mod error;
mod jobs;
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

    let state = AppState::new(db.clone(), &config, superadmin_role_id)?;

    // Spawn delivery worker (no-op until submission_delivery exists).
    jobs::spawn(db.clone(), state.backends.clone());

    let app = router::build(state.clone());

    let listener = tokio::net::TcpListener::bind(&config.listen_addr)
        .await
        .with_context(|| format!("binding {}", config.listen_addr))?;
    tracing::info!(listen = %config.listen_addr, "ready");
    axum::serve(listener, app).await.context("axum::serve")?;

    Ok(())
}
