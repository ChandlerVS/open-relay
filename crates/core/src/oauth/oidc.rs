//! Generic OIDC provider built from an `oauth_provider_config` row.
//!
//! Implements `Provider`. The exchange step calls the token endpoint with
//! PKCE, then fetches the userinfo endpoint (preferred) or falls back to
//! decoding the ID token payload without signature verification — relying on
//! TLS to the token endpoint per OIDC §3.1.3.7.

use async_trait::async_trait;
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use serde_json::Value;

use crate::auth::provider::{Provider, ProviderError, VerifiedIdentity};

const PROVIDER_NAME: &str = "oidc";

#[derive(Clone)]
pub struct OidcProvider {
    pub client_id: String,
    pub client_secret: String,
    pub authorize_url: String,
    pub token_url: String,
    pub userinfo_url: Option<String>,
    pub scopes: Vec<String>,
    pub email_claim: String,
    pub subject_claim: String,
}

impl OidcProvider {
    pub fn from_config(cfg: &entity::oauth_provider_config::Model) -> Self {
        Self {
            client_id: cfg.client_id.clone(),
            client_secret: cfg.client_secret.clone(),
            authorize_url: cfg.authorize_url.clone(),
            token_url: cfg.token_url.clone(),
            userinfo_url: cfg.userinfo_url.clone(),
            scopes: cfg
                .scopes
                .split_whitespace()
                .map(|s| s.to_string())
                .collect(),
            email_claim: cfg.email_claim.clone(),
            subject_claim: cfg.subject_claim.clone(),
        }
    }

    fn build_client(
        &self,
        redirect_uri: &str,
    ) -> Result<BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>, ProviderError>
    {
        let auth_url = AuthUrl::new(self.authorize_url.clone())
            .map_err(|e| ProviderError::Other(format!("authorize_url: {e}")))?;
        let token_url = TokenUrl::new(self.token_url.clone())
            .map_err(|e| ProviderError::Other(format!("token_url: {e}")))?;
        let redirect = RedirectUrl::new(redirect_uri.to_string())
            .map_err(|e| ProviderError::Other(format!("redirect_uri: {e}")))?;

        Ok(BasicClient::new(ClientId::new(self.client_id.clone()))
            .set_client_secret(ClientSecret::new(self.client_secret.clone()))
            .set_auth_uri(auth_url)
            .set_token_uri(token_url)
            .set_redirect_uri(redirect))
    }

    /// Build the authorize URL and return it alongside the PKCE verifier the
    /// caller must stash in the state cookie for the eventual token exchange.
    pub fn authorize_with_pkce(
        &self,
        redirect_uri: &str,
        state_nonce: &str,
    ) -> Result<(String, String), ProviderError> {
        let client = self.build_client(redirect_uri)?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let mut req = client
            .authorize_url(|| CsrfToken::new(state_nonce.to_string()))
            .set_pkce_challenge(challenge);
        for scope in &self.scopes {
            req = req.add_scope(Scope::new(scope.clone()));
        }
        let (url, _csrf) = req.url();
        Ok((url.to_string(), verifier.secret().clone()))
    }
}

#[async_trait]
impl Provider for OidcProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    async fn authorize_url(
        &self,
        redirect_uri: &str,
        state: &str,
    ) -> Result<String, ProviderError> {
        // Helper that doesn't carry the PKCE verifier back; callers who need
        // the verifier should use `authorize_with_pkce`.
        let (url, _verifier) = self.authorize_with_pkce(redirect_uri, state)?;
        Ok(url)
    }

    async fn exchange(
        &self,
        code: &str,
        redirect_uri: &str,
        pkce_verifier: Option<&str>,
    ) -> Result<VerifiedIdentity, ProviderError> {
        let client = self.build_client(redirect_uri)?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| ProviderError::Other(format!("http client: {e}")))?;

        let mut req = client.exchange_code(AuthorizationCode::new(code.to_string()));
        if let Some(verifier) = pkce_verifier {
            req = req.set_pkce_verifier(PkceCodeVerifier::new(verifier.to_string()));
        }
        let token = req
            .request_async(&http)
            .await
            .map_err(|e| ProviderError::Other(format!("token exchange: {e}")))?;

        let access_token = token.access_token().secret();
        let userinfo = self
            .userinfo_url
            .as_deref()
            .ok_or_else(|| ProviderError::Other("userinfo_url not configured".into()))?;

        let claims: Value = http
            .get(userinfo)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::Other(format!("userinfo fetch: {e}")))?
            .error_for_status()
            .map_err(|e| ProviderError::Other(format!("userinfo status: {e}")))?
            .json::<Value>()
            .await
            .map_err(|e| ProviderError::Other(format!("userinfo parse: {e}")))?;

        let subject = claims
            .get(&self.subject_claim)
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::Other(format!(
                    "claim `{}` missing from identity response",
                    self.subject_claim
                ))
            })?
            .to_string();
        let email = claims
            .get(&self.email_claim)
            .and_then(Value::as_str)
            .map(|s| s.to_string());

        Ok(VerifiedIdentity {
            provider: PROVIDER_NAME,
            subject,
            email,
        })
    }
}

