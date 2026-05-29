//! SSO/OAuth extension point.
//!
//! `Provider` is the trait every external identity source implements; the
//! registry holds the live set keyed by provider name (e.g. "google", "okta").
//! No concrete providers are implemented in the skeleton — the `oauth2` crate
//! is in deps and ready to be wired by a specific provider module.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider error: {0}")]
    Other(String),
}

#[async_trait]
pub trait Provider: Send + Sync + 'static {
    fn name(&self) -> &'static str;

    /// Build the URL the user should be redirected to start the OAuth dance.
    async fn authorize_url(&self, redirect_uri: &str, state: &str)
    -> Result<String, ProviderError>;

    /// Exchange the authorization code (returned to our callback) for a
    /// verified email/subject this Provider asserts.
    async fn exchange(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<VerifiedIdentity, ProviderError>;
}

#[derive(Debug, Clone)]
pub struct VerifiedIdentity {
    pub provider: &'static str,
    pub subject: String,
    pub email: Option<String>,
}

#[derive(Default)]
pub struct ProviderRegistry {
    by_name: HashMap<&'static str, Arc<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.by_name.insert(provider.name(), provider);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Provider>> {
        self.by_name.get(name).cloned()
    }
}
