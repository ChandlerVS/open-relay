use std::sync::Arc;

use open_relay_core::auth::AuthKeys;
use open_relay_core::auth::provider::ProviderRegistry;
use open_relay_core::crypto::SecretCipher;
use open_relay_core::backend::registry::BackendRegistry;
use open_relay_core::backend::{GoHighLevelFactory, OpenRelayBackend};
use sea_orm::DatabaseConnection;

use crate::config::{Config, Environment};

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub auth_keys: Arc<AuthKeys>,
    /// AEAD cipher for secret columns at rest. See [`SecretCipher`].
    pub cipher: Arc<SecretCipher>,
    pub providers: Arc<ProviderRegistry>,
    pub backends: BackendRegistry,
    /// Id of the auto-managed `Superadmin` role. Cached at boot via
    /// `rbac::service::ensure_superadmin` so handlers don't need to look it
    /// up — and so the lockout guards (which reference it) can't accidentally
    /// pick up the wrong role if a non-superadmin role happens to be named
    /// "Superadmin" in the DB.
    pub superadmin_role_id: i32,
    pub public_api_url: String,
    /// Absolute URL the embed SDK bundle is served from, used to build the
    /// copy-paste `<script>` snippet handed to form owners. Derived at boot:
    /// `config.embed_sdk_url` when set, else `{public_api_url}/embed/open-relay.js`.
    pub embed_sdk_url: String,
    /// Filesystem path the embed SDK bundle is read from when serving
    /// `GET /embed/open-relay.js`. See [`Config::embed_sdk_path`].
    pub embed_sdk_path: String,
    /// Filesystem path to the built admin SPA, served as the router fallback
    /// when set. See [`Config::admin_dist_path`].
    pub admin_dist_path: Option<String>,
    pub admin_url: String,
    pub cookie_secure: bool,
    pub environment: Environment,
}

impl AppState {
    pub fn new(
        db: DatabaseConnection,
        config: &Config,
        superadmin_role_id: i32,
    ) -> anyhow::Result<Self> {
        let auth_keys = Arc::new(AuthKeys::from_secret(config.jwt_secret.as_bytes()));
        let cipher = Arc::new(SecretCipher::from_base64_key(&config.encryption_key)?);
        let providers = Arc::new(ProviderRegistry::new());
        let mut backends = BackendRegistry::new();
        backends.register_static(Arc::new(OpenRelayBackend));
        backends.register_factory(Arc::new(GoHighLevelFactory::new()));
        let public_api_url = config.public_api_url.trim_end_matches('/').to_string();
        // Fall back to a same-origin path under the API when EMBED_SDK_URL is
        // unset, so the snippet is still well-formed in dev/local setups.
        let embed_sdk_url = match config.embed_sdk_url.trim() {
            "" => format!("{public_api_url}/embed/open-relay.js"),
            url => url.to_string(),
        };
        Ok(Self {
            db,
            auth_keys,
            cipher,
            providers,
            backends,
            superadmin_role_id,
            public_api_url,
            embed_sdk_url,
            embed_sdk_path: config.embed_sdk_path.clone(),
            admin_dist_path: config.admin_dist_path.clone(),
            admin_url: config.admin_url.trim_end_matches('/').to_string(),
            cookie_secure: config.cookie_secure,
            environment: config.environment,
        })
    }

    /// `true` in development. Gates dev-only affordances (Swagger UI, the SSRF
    /// allowance for loopback/private OAuth endpoints).
    pub fn allow_private_network(&self) -> bool {
        self.environment == Environment::Development
    }
}
