//! Generic OIDC provider built from an `oauth_provider_config` row.
//!
//! Implements `Provider`. The exchange step calls the token endpoint with
//! PKCE, then **validates the returned `id_token`** (JWKS signature + `iss` /
//! `aud` / `exp` / `nonce`) and takes the verified `sub` / `email` /
//! `email_verified` from it. The userinfo endpoint is used only as a fallback
//! to fill in `email` when the id_token omits it. This makes the asserted
//! identity cryptographically trustworthy rather than relying on TLS alone.

use async_trait::async_trait;
use oauth2::basic::{
    BasicErrorResponse, BasicTokenIntrospectionResponse, BasicTokenType,
    BasicRevocationErrorResponse,
};
use oauth2::{
    AuthUrl, AuthorizationCode, Client, ClientId, ClientSecret, CsrfToken, EndpointNotSet,
    EndpointSet, ExtraTokenFields, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    StandardRevocableToken, StandardTokenResponse, TokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::provider::{Provider, ProviderError, VerifiedIdentity};
use crate::oauth::idtoken;

const PROVIDER_NAME: &str = "oidc";

/// Extra token-endpoint fields beyond the OAuth2 standard set — we need the
/// OIDC `id_token`, which `BasicTokenResponse` would otherwise discard.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct OidcExtraFields {
    #[serde(default)]
    id_token: Option<String>,
}
impl ExtraTokenFields for OidcExtraFields {}

type OidcTokenResponse = StandardTokenResponse<OidcExtraFields, BasicTokenType>;

/// `BasicClient` with the token-response type swapped for one that retains the
/// `id_token`. Endpoint type-state mirrors `build_client`: auth + token set.
type OidcClient = Client<
    BasicErrorResponse,
    OidcTokenResponse,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
>;

#[derive(Clone)]
pub struct OidcProvider {
    pub client_id: String,
    pub client_secret: String,
    pub issuer: Option<String>,
    pub authorize_url: String,
    pub token_url: String,
    pub userinfo_url: Option<String>,
    pub jwks_url: Option<String>,
    pub scopes: Vec<String>,
    pub email_claim: String,
    pub subject_claim: String,
}

impl OidcProvider {
    pub fn from_config(cfg: &entity::oauth_provider_config::Model) -> Self {
        Self {
            client_id: cfg.client_id.clone(),
            client_secret: cfg.client_secret.clone(),
            issuer: cfg.issuer.clone(),
            authorize_url: cfg.authorize_url.clone(),
            token_url: cfg.token_url.clone(),
            userinfo_url: cfg.userinfo_url.clone(),
            jwks_url: cfg.jwks_url.clone(),
            scopes: cfg
                .scopes
                .split_whitespace()
                .map(|s| s.to_string())
                .collect(),
            email_claim: cfg.email_claim.clone(),
            subject_claim: cfg.subject_claim.clone(),
        }
    }

    fn build_client(&self, redirect_uri: &str) -> Result<OidcClient, ProviderError> {
        let auth_url = AuthUrl::new(self.authorize_url.clone())
            .map_err(|e| ProviderError::Other(format!("authorize_url: {e}")))?;
        let token_url = TokenUrl::new(self.token_url.clone())
            .map_err(|e| ProviderError::Other(format!("token_url: {e}")))?;
        let redirect = RedirectUrl::new(redirect_uri.to_string())
            .map_err(|e| ProviderError::Other(format!("redirect_uri: {e}")))?;

        Ok(Client::new(ClientId::new(self.client_id.clone()))
            .set_client_secret(ClientSecret::new(self.client_secret.clone()))
            .set_auth_uri(auth_url)
            .set_token_uri(token_url)
            .set_redirect_uri(redirect))
    }

    /// Build the authorize URL and return it alongside the PKCE verifier the
    /// caller must stash in the state cookie for the eventual token exchange.
    ///
    /// `state_nonce` is the CSRF `state` value (echoed in the redirect URL);
    /// `oidc_nonce` is the OIDC `nonce` (echoed inside the signed id_token).
    /// They are distinct values bound to the same flow.
    pub fn authorize_with_pkce(
        &self,
        redirect_uri: &str,
        state_nonce: &str,
        oidc_nonce: &str,
    ) -> Result<(String, String), ProviderError> {
        let client = self.build_client(redirect_uri)?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let mut req = client
            .authorize_url(|| CsrfToken::new(state_nonce.to_string()))
            .set_pkce_challenge(challenge)
            .add_extra_param("nonce", oidc_nonce.to_string());
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
        // Convenience that doesn't carry the PKCE verifier back; callers that
        // need the verifier (and a distinct OIDC nonce) use
        // `authorize_with_pkce` directly.
        let (url, _verifier) = self.authorize_with_pkce(redirect_uri, state, state)?;
        Ok(url)
    }

    async fn exchange(
        &self,
        code: &str,
        redirect_uri: &str,
        pkce_verifier: Option<&str>,
        expected_nonce: &str,
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

        // Validate the signed id_token — this is the trust anchor. Both the
        // issuer and JWKS endpoint must be configured; we fail closed if not.
        let issuer = self
            .issuer
            .as_deref()
            .ok_or_else(|| ProviderError::Other("issuer not configured".into()))?;
        let jwks_url = self
            .jwks_url
            .as_deref()
            .ok_or_else(|| ProviderError::Other("jwks_url not configured".into()))?;
        let id_token = token
            .extra_fields()
            .id_token
            .as_deref()
            .ok_or_else(|| ProviderError::Other("provider returned no id_token".into()))?;

        let claims = idtoken::verify(
            &http,
            id_token,
            jwks_url,
            issuer,
            &self.client_id,
            expected_nonce,
        )
        .await
        .map_err(|e| ProviderError::Other(format!("id_token validation: {e}")))?;

        let subject = claims.subject;
        let email_verified = claims.email_verified;
        // Prefer the email from the validated id_token; fall back to userinfo
        // only when the id_token omits it.
        let email = match claims.email {
            Some(e) => Some(e),
            None => self.fetch_userinfo_email(&http, token.access_token().secret()).await?,
        };

        Ok(VerifiedIdentity {
            provider: PROVIDER_NAME,
            subject,
            email,
            email_verified,
        })
    }
}

impl OidcProvider {
    /// Best-effort fetch of the email claim from the userinfo endpoint. Used
    /// only to fill `email` when the id_token didn't carry it.
    async fn fetch_userinfo_email(
        &self,
        http: &reqwest::Client,
        access_token: &str,
    ) -> Result<Option<String>, ProviderError> {
        let Some(userinfo) = self.userinfo_url.as_deref() else {
            return Ok(None);
        };
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
        Ok(claims
            .get(&self.email_claim)
            .and_then(Value::as_str)
            .map(|s| s.to_string()))
    }
}
