use figment::{Figment, providers::Env};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub listen_addr: String,
    pub database_url: String,
    pub jwt_secret: String,
}

impl Config {
    pub fn from_env() -> Result<Self, figment::Error> {
        Figment::new()
            .merge(
                Env::raw()
                    .only(&["LISTEN_ADDR", "DATABASE_URL", "JWT_SECRET"])
                    .map(|k| k.as_str().to_lowercase().into()),
            )
            .extract()
    }
}
