//! Dev helper: mint an API key for a user id.
//!
//! ```text
//! DATABASE_URL=... cargo run -p open-relay-core --example mint_key -- <user_id> [name] [scope,scope,...]
//! ```
use open_relay_core::api_keys::{NewApiKey, service};
use sea_orm::Database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let user_id: i32 = args
        .next()
        .expect("usage: mint_key <user_id> [name]")
        .parse()?;
    let name = args.next().unwrap_or_else(|| "dev".to_string());
    let scopes = args.next().map(|s| {
        s.split(',')
            .filter_map(open_relay_core::permissions::Permission::from_slug)
            .collect::<Vec<_>>()
    });
    let db = Database::connect(&std::env::var("DATABASE_URL")?).await?;
    let created = service::issue(
        &db,
        user_id,
        NewApiKey {
            name,
            scopes,
            expires_in_days: None,
        },
    )
    .await?;
    println!("{}", created.token);
    Ok(())
}
