//! OIDC discovery document fetcher.
//!
//! Used by the admin "Discover" button to prefill provider endpoints from a
//! `.well-known/openid-configuration` URL.

use std::time::Duration;

use serde::Deserialize;
use utoipa::ToSchema;

use crate::error::{CoreError, CoreResult};

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Deserialize, ToSchema, serde::Serialize)]
pub struct DiscoveryDocument {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub userinfo_endpoint: Option<String>,
    #[serde(default)]
    pub jwks_uri: Option<String>,
    #[serde(default)]
    pub scopes_supported: Option<Vec<String>>,
}

pub async fn fetch_discovery(url: &str) -> CoreResult<DiscoveryDocument> {
    let client = reqwest::Client::builder()
        .timeout(DISCOVERY_TIMEOUT)
        .build()
        .map_err(|e| CoreError::OAuthDiscoveryFailed(format!("client init: {e}")))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| CoreError::OAuthDiscoveryFailed(format!("fetch: {e}")))?;

    if !resp.status().is_success() {
        return Err(CoreError::OAuthDiscoveryFailed(format!(
            "discovery endpoint returned {}",
            resp.status()
        )));
    }

    let doc: DiscoveryDocument = resp
        .json()
        .await
        .map_err(|e| CoreError::OAuthDiscoveryFailed(format!("parse: {e}")))?;

    if doc.issuer.is_empty()
        || doc.authorization_endpoint.is_empty()
        || doc.token_endpoint.is_empty()
    {
        return Err(CoreError::OAuthDiscoveryFailed(
            "discovery document missing required fields".into(),
        ));
    }

    Ok(doc)
}
