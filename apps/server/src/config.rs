use figment::{Figment, providers::Env};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub listen_addr: String,
    pub database_url: String,
    pub jwt_secret: String,
    /// Fully-qualified base URL the API is reachable at from end-user browsers.
    /// Used to construct the OAuth redirect_uri the IdP whitelists.
    /// e.g. `http://localhost:8080` for dev, `https://api.example.com` in prod.
    pub public_api_url: String,
    /// Fully-qualified base URL the admin SPA is reachable at. Used as the
    /// post-OAuth-callback redirect target.
    pub admin_url: String,
    /// Whether to set the `Secure` attribute on the OAuth state cookie.
    /// Must be false when serving the API over plain HTTP locally.
    #[serde(default = "default_cookie_secure")]
    pub cookie_secure: bool,
}

fn default_cookie_secure() -> bool {
    true
}

impl Config {
    pub fn from_env() -> Result<Self, figment::Error> {
        Figment::new()
            .merge(
                Env::raw()
                    .only(&[
                        "LISTEN_ADDR",
                        "DATABASE_URL",
                        "JWT_SECRET",
                        "PUBLIC_API_URL",
                        "ADMIN_URL",
                        "COOKIE_SECURE",
                    ])
                    .map(|k| k.as_str().to_lowercase().into()),
            )
            .extract()
    }
}
