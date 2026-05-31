use anyhow::{Context, bail};
use axum::http::HeaderValue;
use figment::{Figment, providers::Env};
use serde::Deserialize;

/// Minimum acceptable length, in bytes, for `JWT_SECRET`. HS256 derives its
/// security from the secret's entropy; anything shorter than the 256-bit
/// (32-byte) digest is trivially weak.
const MIN_JWT_SECRET_LEN: usize = 32;

/// Low-entropy placeholders that ship in examples/tutorials. Refusing these
/// (case-insensitively) stops a copy-pasted `.env` from booting with a
/// publicly-known signing key. Compared after trimming.
const JWT_SECRET_PLACEHOLDERS: &[&str] = &[
    "dev-only-change-me",
    "change_me",
    "change-me",
    "changeme",
    "secret",
    "password",
];

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
    pub fn from_env() -> anyhow::Result<Self> {
        let config: Self = Figment::new()
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
            .context("extracting environment configuration")?;
        config.validate()?;
        Ok(config)
    }

    /// Fail closed at boot on insecure/malformed values, rather than degrading
    /// silently at runtime (a weak signing key, or a CORS origin that can't be
    /// enforced).
    fn validate(&self) -> anyhow::Result<()> {
        let secret = self.jwt_secret.trim();
        if JWT_SECRET_PLACEHOLDERS
            .iter()
            .any(|p| p.eq_ignore_ascii_case(secret))
        {
            bail!(
                "JWT_SECRET is set to a known placeholder value; replace it with a unique \
                 secret (generate one with `openssl rand -base64 32`)"
            );
        }
        if secret.len() < MIN_JWT_SECRET_LEN {
            bail!(
                "JWT_SECRET must be at least {MIN_JWT_SECRET_LEN} bytes of high-entropy \
                 random data (generate one with `openssl rand -base64 32`)"
            );
        }

        // These become CORS / redirect origins. Validate now so the parse in
        // the router can't fail open. The router uses the trimmed form.
        validate_origin("ADMIN_URL", &self.admin_url)?;
        validate_origin("PUBLIC_API_URL", &self.public_api_url)?;
        Ok(())
    }
}

/// Ensure a configured base URL is non-empty and usable as an HTTP header value
/// (the form a CORS `Access-Control-Allow-Origin` / `Location` header takes),
/// matching the trimming the router/state apply.
fn validate_origin(name: &str, value: &str) -> anyhow::Result<()> {
    let trimmed = value.trim_end_matches('/');
    if trimmed.is_empty() {
        bail!("{name} must be set to a fully-qualified origin (e.g. https://app.example.com)");
    }
    // A CORS `Origin` never carries whitespace/control chars; `HeaderValue`
    // itself permits interior spaces, so reject them here (the trailing-space
    // fat-finger the original permissive fallback would have swallowed).
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        bail!("{name} must not contain whitespace or control characters: {value:?}");
    }
    HeaderValue::from_str(trimmed)
        .with_context(|| format!("{name} is not a valid origin: {value:?}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(jwt_secret: &str, admin_url: &str) -> Config {
        Config {
            listen_addr: "0.0.0.0:8080".into(),
            database_url: "mysql://localhost/db".into(),
            jwt_secret: jwt_secret.into(),
            public_api_url: "http://localhost:8080".into(),
            admin_url: admin_url.into(),
            cookie_secure: true,
        }
    }

    const STRONG_SECRET: &str = "Qm9vdHN0cmFwLXN0cm9uZy1zZWNyZXQtMzJieXRlcyE";

    #[test]
    fn accepts_strong_secret_and_valid_origin() {
        assert!(config_with(STRONG_SECRET, "http://localhost:5173").validate().is_ok());
    }

    #[test]
    fn rejects_short_secret() {
        assert!(config_with("too-short", "http://localhost:5173").validate().is_err());
    }

    #[test]
    fn rejects_placeholder_secret_case_insensitive() {
        assert!(config_with("dev-only-change-me", "http://localhost:5173").validate().is_err());
        assert!(config_with("DEV-ONLY-CHANGE-ME", "http://localhost:5173").validate().is_err());
    }

    #[test]
    fn rejects_malformed_admin_url() {
        // A space is not a legal header-value byte.
        assert!(config_with(STRONG_SECRET, "http://localhost:5173 ").validate().is_err());
        assert!(config_with(STRONG_SECRET, "").validate().is_err());
    }
}
