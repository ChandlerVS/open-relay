use std::sync::Arc;

use open_relay_core::backend::registry::BackendRegistry;
use sea_orm::DatabaseConnection;

use crate::auth::AuthKeys;
use crate::auth::provider::ProviderRegistry;
use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub auth_keys: Arc<AuthKeys>,
    pub providers: Arc<ProviderRegistry>,
    pub backends: BackendRegistry,
}

impl AppState {
    pub fn new(db: DatabaseConnection, config: &Config) -> anyhow::Result<Self> {
        let auth_keys = Arc::new(AuthKeys::from_secret(config.jwt_secret.as_bytes()));
        let providers = Arc::new(ProviderRegistry::new());
        let backends = BackendRegistry::new();
        Ok(Self {
            db,
            auth_keys,
            providers,
            backends,
        })
    }
}
