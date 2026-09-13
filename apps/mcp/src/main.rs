//! `open-relay-mcp` — the stdio MCP server.
//!
//! Talks to MySQL directly through `open_relay_core`, with no HTTP hop. That
//! makes it the right shape for an agent running on the same host as the
//! database — a developer's machine, or a server-side automation — and the
//! wrong shape for anything remote, which should use the server's `/api/v1/mcp`
//! endpoint instead.
//!
//! Configuration is three environment variables:
//!
//! ```text
//! DATABASE_URL        mysql://root:openrelay@127.0.0.1:3306/openrelay
//! ENCRYPTION_KEY      base64 32-byte key (same value the server uses)
//! OPEN_RELAY_API_KEY  orl_... minted at /profile in the admin UI
//! ```
//!
//! In an MCP client config:
//!
//! ```json
//! { "command": "open-relay-mcp", "env": { "DATABASE_URL": "...", "OPEN_RELAY_API_KEY": "orl_..." } }
//! ```

use anyhow::Context;
use open_relay_core::api_keys::service as api_keys;
use open_relay_core::backend::BackendRegistry;
use open_relay_core::backend::gohighlevel::GoHighLevelFactory;
use open_relay_core::backend::openrelay::OpenRelayBackend;
use open_relay_mcp::handler::OpenRelayMcp;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use sea_orm::Database;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    // Stdout is the protocol channel — anything written there that is not
    // JSON-RPC corrupts the stream and the client disconnects with a parse
    // error that points nowhere useful. All logging goes to stderr.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let api_key = std::env::var("OPEN_RELAY_API_KEY").context(
        "OPEN_RELAY_API_KEY is required — mint one at /profile in the OpenRelay admin UI",
    )?;

    let db = Database::connect(&database_url)
        .await
        .context("connecting to MySQL")?;

    // Resolved once, at startup, rather than per call: there is exactly one
    // caller for the life of the process, and failing here gives a legible
    // error on the client's stderr instead of an authorization failure on
    // every tool call.
    let actor = api_keys::authenticate(&db, &api_key)
        .await
        .context("OPEN_RELAY_API_KEY was not accepted (unknown, revoked, or expired)")?;
    tracing::info!(
        user_id = actor.user_id,
        permissions = actor.permissions.len(),
        "authenticated"
    );

    // Mirrors `AppState::new` — `create_form`/`update_form` validate a form's
    // backend bindings against this registry, so a binding the server accepts
    // has to be accepted here too.
    let mut backends = BackendRegistry::new();
    backends.register_static(Arc::new(OpenRelayBackend));
    backends.register_factory(Arc::new(GoHighLevelFactory::new()));

    let service = OpenRelayMcp::stdio(db, backends, actor)
        .serve(stdio())
        .await
        .inspect_err(|err| tracing::error!(?err, "failed to start stdio transport"))?;
    service.waiting().await?;
    Ok(())
}
