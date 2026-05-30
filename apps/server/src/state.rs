use std::sync::Arc;

use open_relay_core::auth::AuthKeys;
use open_relay_core::auth::provider::ProviderRegistry;
use open_relay_core::backend::registry::BackendRegistry;
use open_relay_core::backend::OpenRelayBackend;
use sea_orm::DatabaseConnection;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub auth_keys: Arc<AuthKeys>,
    pub providers: Arc<ProviderRegistry>,
    pub backends: BackendRegistry,
    /// Id of the auto-managed `Superadmin` role. Cached at boot via
    /// `rbac::service::ensure_superadmin` so handlers don't need to look it
    /// up — and so the lockout guards (which reference it) can't accidentally
    /// pick up the wrong role if a non-superadmin role happens to be named
    /// "Superadmin" in the DB.
    pub superadmin_role_id: i32,
    pub public_api_url: String,
    pub admin_url: String,
    pub cookie_secure: bool,
}

impl AppState {
    pub fn new(
        db: DatabaseConnection,
        config: &Config,
        superadmin_role_id: i32,
    ) -> anyhow::Result<Self> {
        let auth_keys = Arc::new(AuthKeys::from_secret(config.jwt_secret.as_bytes()));
        let providers = Arc::new(ProviderRegistry::new());
        let mut backends = BackendRegistry::new();
        backends.register(Arc::new(OpenRelayBackend));
        Ok(Self {
            db,
            auth_keys,
            providers,
            backends,
            superadmin_role_id,
            public_api_url: config.public_api_url.trim_end_matches('/').to_string(),
            admin_url: config.admin_url.trim_end_matches('/').to_string(),
            cookie_secure: config.cookie_secure,
        })
    }
}
