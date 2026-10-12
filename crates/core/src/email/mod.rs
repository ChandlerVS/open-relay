//! Outbound email: the pluggable surface that sends submission notifications
//! (AWS SES today).
//!
//! Mirrors [`crate::storage`] deliberately — an [`EmailSenderFactory`] is built
//! from an `email_provider` DB row exactly the way a `FileStoreFactory` is
//! built from a `storage_provider` row, and [`EmailSenderFactory::secret_keys`]
//! plays the same three-jobs-from-one-declaration role: DTO redaction,
//! preservation across partial updates, and encrypt/decrypt at rest.
//!
//! Every kind needs credentials, so every kind is factory-built; there is no
//! static-singleton flavour.
//!
//! # Plain text only, and visitor data stays out of headers
//!
//! [`OutgoingEmail`] has no HTML body on purpose. Notification bodies carry
//! values typed by anonymous visitors, and a plain-text body has no markup to
//! inject into. The recipient is admin-configured and checked by
//! [`validate_address`], which refuses anything that could smuggle a second
//! address or a header (commas, semicolons, CR/LF, angle brackets).

pub mod registry;
pub mod ses;

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;

pub use registry::{EmailKindInfo, EmailRegistry};
pub use ses::{SesConfig, SesFactory};

/// RFC 5321's practical ceiling on a forward path.
pub const MAX_ADDRESS_LEN: usize = 254;
/// RFC 5321's ceiling on the local part.
const MAX_LOCAL_LEN: usize = 64;

/// A single plain-text message to one recipient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingEmail {
    pub to: String,
    pub subject: String,
    pub text: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EmailError {
    /// The provider is unreachable or throttling us. Worth retrying.
    #[error("email provider unavailable: {0}")]
    Transient(String),
    /// The provider refused the message for a reason a retry won't fix
    /// (unverified sender, sandbox recipient, bad credentials).
    #[error("email rejected: {0}")]
    Permanent(String),
    /// The message can't be built as asked (malformed address, …).
    #[error("email invalid: {0}")]
    Invalid(String),
}

impl EmailError {
    /// The admin-facing message without the variant prefix.
    pub fn message(&self) -> &str {
        match self {
            Self::Transient(m) | Self::Permanent(m) | Self::Invalid(m) => m,
        }
    }
}

/// Returned by a factory when the stored config can't be parsed/validated.
#[derive(Debug, Error)]
pub enum EmailBuildError {
    #[error("invalid email config: {0}")]
    Invalid(String),
}

#[async_trait]
pub trait EmailSender: Send + Sync + 'static {
    /// Stable kind identifier, shared with the `email_provider.kind` column.
    fn kind(&self) -> &'static str;

    /// Send one message. Not idempotent — no provider offers a dedup key — so
    /// a retry after a lost response can deliver twice.
    async fn send(&self, email: &OutgoingEmail) -> Result<(), EmailError>;
}

/// Builds a configured [`EmailSender`] from a stored `email_provider` row.
pub trait EmailSenderFactory: Send + Sync + 'static {
    /// Stable kind identifier shared with the `email_provider.kind` column.
    fn kind(&self) -> &'static str;

    /// Human label for the admin's provider picker.
    fn label(&self) -> &'static str;

    /// Top-level keys in this kind's `config` JSON that hold secrets. These are
    /// redacted from admin-facing DTOs, preserved across partial updates that
    /// omit them, and encrypted at rest. Default: none.
    fn secret_keys(&self) -> &'static [&'static str] {
        &[]
    }

    /// Parse + validate the row's `config` JSON and yield a ready-to-call
    /// sender. Called on every write (as validation) and on every delivery,
    /// so implementations should stay cheap — clone a shared
    /// `reqwest::Client` rather than build a new pool.
    fn build(&self, config: &Value) -> Result<Arc<dyn EmailSender>, EmailBuildError>;
}

/// Validate a single bare email address (`local@domain`) and return it
/// trimmed. Deliberately stricter than RFC 5322: no display name, no quoting,
/// no comments, no lists. Anything that could carry a second recipient or a
/// header break — whitespace, control characters, `,` `;` `<` `>` `"` `(` `)`
/// `[` `]` `\` — is refused, so the value can be handed to a provider (or
/// formatted into a header) without escaping.
pub fn validate_address(raw: &str) -> Result<String, String> {
    let addr = raw.trim();
    if addr.is_empty() {
        return Err("email address is empty".into());
    }
    if addr.len() > MAX_ADDRESS_LEN {
        return Err(format!(
            "email address must be at most {MAX_ADDRESS_LEN} characters"
        ));
    }
    if addr
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || ",;<>\"()[]\\".contains(c))
    {
        return Err("email address must be a single plain address like name@example.com".into());
    }
    let mut parts = addr.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err("email address must contain exactly one '@'".into());
    };
    if local.is_empty() || local.len() > MAX_LOCAL_LEN {
        return Err("email address has an invalid local part".into());
    }
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2
        || labels
            .iter()
            .any(|l| l.is_empty() || l.starts_with('-') || l.ends_with('-'))
    {
        return Err("email address has an invalid domain".into());
    }
    Ok(addr.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_and_trims_plain_addresses() {
        assert_eq!(
            validate_address("  ops@example.com ").unwrap(),
            "ops@example.com"
        );
        assert!(validate_address("first.last+tag@sub.example.co.uk").is_ok());
    }

    #[test]
    fn rejects_lists_and_header_breaks() {
        for bad in [
            "a@example.com,b@example.com",
            "a@example.com;b@example.com",
            "a@example.com\r\nBcc: x@evil.com",
            "a@exa mple.com",
            "Ops <ops@example.com>",
            "\"a\"@example.com",
        ] {
            assert!(validate_address(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn rejects_malformed_addresses() {
        for bad in [
            "",
            "   ",
            "no-at-sign",
            "@example.com",
            "a@",
            "a@b@example.com",
            "a@localhost",
            "a@.example.com",
            "a@example..com",
            "a@-example.com",
        ] {
            assert!(validate_address(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn rejects_overlong_addresses() {
        let long = format!("{}@example.com", "a".repeat(MAX_ADDRESS_LEN));
        assert!(validate_address(&long).is_err());
        let long_local = format!("{}@example.com", "a".repeat(MAX_LOCAL_LEN + 1));
        assert!(validate_address(&long_local).is_err());
    }
}
