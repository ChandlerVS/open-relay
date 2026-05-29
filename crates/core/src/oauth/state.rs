//! Signed OAuth flow state envelope.
//!
//! During the authorize → callback round-trip we need to bind:
//!   - the random `state` nonce echoed by the IdP (CSRF defense),
//!   - the PKCE verifier (proof-of-possession on the token exchange),
//!   - the flow mode (sign-in vs link to an existing user),
//!   - an absolute expiry.
//!
//! We serialize all of that, HMAC-sign it with the JWT secret, and stash it
//! in a short-lived HttpOnly cookie. The callback reads the cookie, verifies
//! the signature, and matches the embedded nonce against the `state` query
//! param echoed by the IdP.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::auth::AuthKeys;
use crate::error::{CoreError, CoreResult};

const STATE_TTL_SECONDS: i64 = 600;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OAuthMode {
    SignIn,
    Link { user_id: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthFlowState {
    pub nonce: String,
    pub pkce_verifier: String,
    pub mode: OAuthMode,
    pub expires_at: i64,
}

impl OAuthFlowState {
    pub fn new(mode: OAuthMode, pkce_verifier: String) -> Self {
        Self {
            nonce: random_nonce(),
            pkce_verifier,
            mode,
            expires_at: chrono::Utc::now().timestamp() + STATE_TTL_SECONDS,
        }
    }
}

pub fn random_nonce() -> String {
    let mut buf = [0u8; 32];
    rand::rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

/// Serialize the flow state, sign it, return the cookie value
/// (`base64(json).base64(hmac)`).
pub fn issue_state(keys: &AuthKeys, payload: &OAuthFlowState) -> CoreResult<String> {
    let json = serde_json::to_vec(payload)
        .map_err(|e| CoreError::Internal(anyhow::anyhow!("state encode: {e}")))?;
    let encoded_payload = URL_SAFE_NO_PAD.encode(&json);
    let mac = sign(keys, encoded_payload.as_bytes())?;
    Ok(format!("{}.{}", encoded_payload, URL_SAFE_NO_PAD.encode(mac)))
}

/// Verify a cookie value and ensure its nonce matches the `state` query param
/// the IdP echoed back. Also enforces the expiry window.
pub fn verify_state(
    keys: &AuthKeys,
    cookie_value: &str,
    expected_nonce: &str,
) -> CoreResult<OAuthFlowState> {
    let (encoded_payload, encoded_mac) = cookie_value
        .split_once('.')
        .ok_or(CoreError::OAuthStateMismatch)?;
    let mac = URL_SAFE_NO_PAD
        .decode(encoded_mac)
        .map_err(|_| CoreError::OAuthStateMismatch)?;

    let expected_mac = sign(keys, encoded_payload.as_bytes())?;
    if !constant_time_eq(&mac, &expected_mac) {
        return Err(CoreError::OAuthStateMismatch);
    }

    let json = URL_SAFE_NO_PAD
        .decode(encoded_payload)
        .map_err(|_| CoreError::OAuthStateMismatch)?;
    let payload: OAuthFlowState =
        serde_json::from_slice(&json).map_err(|_| CoreError::OAuthStateMismatch)?;

    if payload.nonce != expected_nonce {
        return Err(CoreError::OAuthStateMismatch);
    }
    if chrono::Utc::now().timestamp() > payload.expires_at {
        return Err(CoreError::OAuthStateMismatch);
    }

    Ok(payload)
}

fn sign(keys: &AuthKeys, data: &[u8]) -> CoreResult<Vec<u8>> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(keys.hmac_secret())
        .map_err(|e| CoreError::Internal(anyhow::anyhow!("hmac init: {e}")))?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> AuthKeys {
        AuthKeys::from_secret(b"test-secret-state")
    }

    #[test]
    fn round_trip() {
        let k = keys();
        let payload = OAuthFlowState::new(OAuthMode::SignIn, "verifier-xyz".into());
        let nonce = payload.nonce.clone();
        let cookie = issue_state(&k, &payload).unwrap();
        let parsed = verify_state(&k, &cookie, &nonce).unwrap();
        assert_eq!(parsed.pkce_verifier, "verifier-xyz");
        assert_eq!(parsed.mode, OAuthMode::SignIn);
    }

    #[test]
    fn rejects_tampered_payload() {
        let k = keys();
        let payload = OAuthFlowState::new(OAuthMode::SignIn, "verifier".into());
        let nonce = payload.nonce.clone();
        let cookie = issue_state(&k, &payload).unwrap();
        let (_, sig) = cookie.split_once('.').unwrap();
        let tampered = format!("{}.{}", URL_SAFE_NO_PAD.encode(b"{}"), sig);
        assert!(verify_state(&k, &tampered, &nonce).is_err());
    }

    #[test]
    fn rejects_wrong_nonce() {
        let k = keys();
        let payload = OAuthFlowState::new(OAuthMode::SignIn, "verifier".into());
        let cookie = issue_state(&k, &payload).unwrap();
        assert!(verify_state(&k, &cookie, "other-nonce").is_err());
    }

    #[test]
    fn rejects_expired() {
        let k = keys();
        let mut payload = OAuthFlowState::new(OAuthMode::SignIn, "verifier".into());
        payload.expires_at = chrono::Utc::now().timestamp() - 1;
        let nonce = payload.nonce.clone();
        let cookie = issue_state(&k, &payload).unwrap();
        assert!(verify_state(&k, &cookie, &nonce).is_err());
    }
}
