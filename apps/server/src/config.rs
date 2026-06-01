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

/// Required raw length, in bytes, of the decoded `ENCRYPTION_KEY`. This is the
/// key size for the XChaCha20-Poly1305 AEAD used to encrypt secrets at rest.
const ENCRYPTION_KEY_LEN: usize = 32;

/// Deployment environment. Gates affordances that are safe in local
/// development but dangerous in production: the unauthenticated Swagger
/// UI / `/openapi.json`, the SSRF allowance for loopback/private OAuth
/// endpoints, and HSTS emission. Defaults to [`Environment::Production`]
/// so an unset or typo'd value fails *safe* rather than opening the surface.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Development,
    #[default]
    Production,
}

fn default_environment() -> Environment {
    Environment::Production
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub listen_addr: String,
    pub database_url: String,
    pub jwt_secret: String,
    /// AEAD key (base64) used to encrypt secret columns at rest
    /// (`oauth_provider_config.client_secret`, backend instance tokens).
    /// Separate from `jwt_secret`; must decode to exactly 32 bytes.
    pub encryption_key: String,
    /// Deployment environment. See [`Environment`].
    #[serde(default = "default_environment")]
    pub environment: Environment,
    /// Fully-qualified base URL the API is reachable at from end-user browsers.
    /// Used to construct the OAuth redirect_uri the IdP whitelists.
    /// e.g. `http://localhost:8080` for dev, `https://api.example.com` in prod.
    pub public_api_url: String,
    /// Fully-qualified base URL the admin SPA is reachable at. Used as the
    /// post-OAuth-callback redirect target.
    pub admin_url: String,
    /// Fully-qualified URL the embed SDK bundle (`open-relay.js`) is served
    /// from. Host pages load it via `<script src="…">`, and the admin surfaces
    /// a copy-paste snippet built from it. Optional: when blank, it defaults to
    /// `{public_api_url}/embed/open-relay.js` (see [`AppState::new`]).
    #[serde(default)]
    pub embed_sdk_url: String,
    /// Filesystem path to the built embed SDK bundle, served at
    /// `GET /embed/open-relay.js`. Relative paths resolve against the process
    /// working directory (the repo root under `cargo run`). Defaults to the
    /// Vite build output so the snippet works out of the box in local dev.
    #[serde(default = "default_embed_sdk_path")]
    pub embed_sdk_path: String,
    /// Whether to set the `Secure` attribute on the OAuth state cookie.
    /// Must be false when serving the API over plain HTTP locally.
    #[serde(default = "default_cookie_secure")]
    pub cookie_secure: bool,
}

fn default_cookie_secure() -> bool {
    true
}

fn default_embed_sdk_path() -> String {
    "apps/embed-sdk/dist/open-relay.js".to_string()
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
                        "ENCRYPTION_KEY",
                        "ENVIRONMENT",
                        "PUBLIC_API_URL",
                        "ADMIN_URL",
                        "EMBED_SDK_URL",
                        "EMBED_SDK_PATH",
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

        // ENCRYPTION_KEY must be a distinct, full-strength AEAD key. Reusing
        // JWT_SECRET (or a low-entropy placeholder) would tie secret-at-rest
        // confidentiality to the token-signing key. Require it to base64-decode
        // to exactly 32 bytes — fail closed at boot, never silently plaintext.
        let enc_key = self.encryption_key.trim();
        if JWT_SECRET_PLACEHOLDERS
            .iter()
            .any(|p| p.eq_ignore_ascii_case(enc_key))
        {
            bail!(
                "ENCRYPTION_KEY is set to a known placeholder value; replace it with a unique \
                 key (generate one with `openssl rand -base64 32`)"
            );
        }
        if enc_key.eq_ignore_ascii_case(secret) {
            bail!("ENCRYPTION_KEY must differ from JWT_SECRET (use a separate key)");
        }
        match base64_decode(enc_key) {
            Some(bytes) if bytes.len() == ENCRYPTION_KEY_LEN => {}
            _ => bail!(
                "ENCRYPTION_KEY must be base64 that decodes to exactly {ENCRYPTION_KEY_LEN} bytes \
                 (generate one with `openssl rand -base64 32`)"
            ),
        }

        // These become CORS / redirect origins. Validate now so the parse in
        // the router can't fail open. The router uses the trimmed form.
        validate_origin("ADMIN_URL", &self.admin_url)?;
        validate_origin("PUBLIC_API_URL", &self.public_api_url)?;
        Ok(())
    }
}

/// Standard-alphabet base64 decode (with or without padding), used only to
/// length-check `ENCRYPTION_KEY` at boot.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(s))
        .ok()
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

    /// Valid base64 for exactly 32 bytes, computed at runtime to avoid a
    /// hand-miscounted literal.
    fn strong_encryption_key() -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode([0u8; ENCRYPTION_KEY_LEN])
    }

    fn config_with(jwt_secret: &str, admin_url: &str) -> Config {
        Config {
            listen_addr: "0.0.0.0:8080".into(),
            database_url: "mysql://localhost/db".into(),
            jwt_secret: jwt_secret.into(),
            encryption_key: strong_encryption_key(),
            environment: Environment::Production,
            public_api_url: "http://localhost:8080".into(),
            admin_url: admin_url.into(),
            embed_sdk_url: String::new(),
            embed_sdk_path: default_embed_sdk_path(),
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

    #[test]
    fn accepts_valid_encryption_key() {
        assert!(config_with(STRONG_SECRET, "http://localhost:5173").validate().is_ok());
    }

    #[test]
    fn rejects_encryption_key_wrong_length() {
        use base64::Engine;
        let short = base64::engine::general_purpose::STANDARD.encode([0u8; 16]);
        let mut cfg = config_with(STRONG_SECRET, "http://localhost:5173");
        cfg.encryption_key = short;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_encryption_key_not_base64() {
        let mut cfg = config_with(STRONG_SECRET, "http://localhost:5173");
        cfg.encryption_key = "not valid base64 !!!".into();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_encryption_key_equal_to_jwt_secret() {
        let mut cfg = config_with(STRONG_SECRET, "http://localhost:5173");
        cfg.encryption_key = STRONG_SECRET.into();
        cfg.jwt_secret = STRONG_SECRET.into();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_placeholder_encryption_key() {
        let mut cfg = config_with(STRONG_SECRET, "http://localhost:5173");
        cfg.encryption_key = "change_me".into();
        assert!(cfg.validate().is_err());
    }
}
