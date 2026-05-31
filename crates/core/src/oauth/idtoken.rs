//! OIDC ID-token validation.
//!
//! The token-endpoint response carries a signed `id_token` (a JWT). Unlike the
//! userinfo body — which is just an HTTP response we'd be trusting on TLS alone
//! — the ID token is cryptographically bound to the provider's signing key, so
//! validating it gives us a trustworthy `sub`/`email`/`email_verified`.
//!
//! We verify, per OIDC Core §3.1.3.7:
//!   - the RS256 signature against the provider's JWKS (matched by `kid`),
//!   - `iss` equals the configured issuer,
//!   - `aud` contains our `client_id`,
//!   - `exp` is in the future (jsonwebtoken default),
//!   - `nonce` equals the value we planted in the authorize request.

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdTokenError {
    #[error("fetching JWKS: {0}")]
    Jwks(String),
    #[error("malformed id_token: {0}")]
    Malformed(String),
    #[error("no JWKS key matched the id_token `kid`")]
    NoMatchingKey,
    #[error("id_token signature/claims invalid: {0}")]
    Invalid(String),
    #[error("id_token nonce mismatch")]
    NonceMismatch,
}

/// The subset of validated ID-token claims OpenRelay consumes.
#[derive(Debug, Clone)]
pub struct IdTokenClaims {
    pub subject: String,
    pub email: Option<String>,
    /// `Some(true)`/`Some(false)` when the `email_verified` claim is present,
    /// `None` when absent (some IdPs, e.g. Microsoft Entra, never send it).
    pub email_verified: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawClaims {
    sub: String,
    #[serde(default)]
    iss: Option<String>,
    // Microsoft Entra tenant id, used to resolve the `{tenantid}` issuer
    // template on the multi-tenant (`common`/`organizations`) endpoints.
    #[serde(default)]
    tid: Option<String>,
    #[serde(default)]
    email: Option<String>,
    // `email_verified` is a bool per spec but some IdPs send the string
    // "true"/"false"; accept either.
    #[serde(default)]
    email_verified: Option<serde_json::Value>,
    #[serde(default)]
    nonce: Option<String>,
}

/// Fetch the provider's JWKS and fully validate `id_token` against it.
///
/// `issuer`, `client_id`, and `expected_nonce` are matched against the token's
/// `iss`, `aud`, and `nonce` claims respectively.
pub async fn verify(
    http: &reqwest::Client,
    id_token: &str,
    jwks_url: &str,
    issuer: &str,
    client_id: &str,
    expected_nonce: &str,
) -> Result<IdTokenClaims, IdTokenError> {
    let jwks: JwkSet = http
        .get(jwks_url)
        .send()
        .await
        .map_err(|e| IdTokenError::Jwks(e.to_string()))?
        .error_for_status()
        .map_err(|e| IdTokenError::Jwks(e.to_string()))?
        .json()
        .await
        .map_err(|e| IdTokenError::Jwks(e.to_string()))?;
    verify_with_jwks(&jwks, id_token, issuer, client_id, expected_nonce)
}

/// Crypto core of [`verify`]: validate `id_token` against an already-fetched
/// JWKS. Split out so it can be unit-tested without a network round-trip.
pub fn verify_with_jwks(
    jwks: &JwkSet,
    id_token: &str,
    issuer: &str,
    client_id: &str,
    expected_nonce: &str,
) -> Result<IdTokenClaims, IdTokenError> {
    let header =
        decode_header(id_token).map_err(|e| IdTokenError::Malformed(e.to_string()))?;

    // Only asymmetric algorithms are acceptable. Pinning to RS256 (the OIDC
    // default, used by Google/Okta/Azure/Auth0/…) also blocks the classic
    // algorithm-confusion attack where an attacker forces HS256 and signs with
    // the public key as the HMAC secret.
    if header.alg != Algorithm::RS256 {
        return Err(IdTokenError::Invalid(format!(
            "unsupported id_token alg {:?}; expected RS256",
            header.alg
        )));
    }

    let jwk = match header.kid.as_deref() {
        Some(kid) => jwks.find(kid).ok_or(IdTokenError::NoMatchingKey)?,
        // No `kid` in the header — only unambiguous when the set has one key.
        None => match jwks.keys.as_slice() {
            [only] => only,
            _ => return Err(IdTokenError::NoMatchingKey),
        },
    };

    let key = DecodingKey::from_jwk(jwk).map_err(|e| IdTokenError::Invalid(e.to_string()))?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[client_id]);
    // `iss` is validated manually below (jsonwebtoken only does exact match,
    // which can't express Entra's `{tenantid}` template). exp is validated by
    // default; require all three spec claims to be present.
    validation.set_required_spec_claims(&["exp", "iss", "aud"]);

    let data = decode::<RawClaims>(id_token, &key, &validation)
        .map_err(|e| IdTokenError::Invalid(e.to_string()))?;
    let claims = data.claims;

    // Issuer match. Microsoft Entra's multi-tenant discovery advertises an
    // issuer carrying a literal `{tenantid}` placeholder; the token's real
    // `iss` substitutes the concrete tenant (its `tid` claim). Resolve the
    // template before comparing; for every other provider this is a plain
    // exact match.
    let expected_issuer = match claims.tid.as_deref() {
        Some(tid) if issuer.contains("{tenantid}") => issuer.replace("{tenantid}", tid),
        _ => issuer.to_string(),
    };
    let token_iss = claims.iss.as_deref().unwrap_or_default();
    if token_iss != expected_issuer {
        return Err(IdTokenError::Invalid(format!(
            "issuer mismatch: token `iss` is {token_iss:?}, expected {expected_issuer:?}"
        )));
    }

    // Nonce binds this token to the authorize request we initiated.
    match claims.nonce.as_deref() {
        Some(n) if n == expected_nonce => {}
        _ => return Err(IdTokenError::NonceMismatch),
    }

    Ok(IdTokenClaims {
        subject: claims.sub,
        email: claims.email,
        email_verified: parse_bool(claims.email_verified.as_ref()),
    })
}

/// Interpret an `email_verified` claim that may be a JSON bool or the strings
/// "true"/"false". Returns `None` when the claim is absent/null/unrecognized.
fn parse_bool(v: Option<&serde_json::Value>) -> Option<bool> {
    match v {
        Some(serde_json::Value::Bool(b)) => Some(*b),
        Some(serde_json::Value::String(s)) if s.eq_ignore_ascii_case("true") => Some(true),
        Some(serde_json::Value::String(s)) if s.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use serde::Serialize;

    const ISSUER: &str = "https://idp.example.com";
    const CLIENT_ID: &str = "test-client";
    const NONCE: &str = "nonce-abc123";

    // RSA-2048 test keypair (fixture only — never used outside tests).
    const PRIVATE_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDtSAIa0UJL6Zm3\n\
aQHkWa5O5Zm19hrepfbSZRC3sMx3cMNJGz58dhnFT3fHTXa8C9YXGD+FM3TwCDym\n\
NNcpwugCTu0LqNwJ6nviAWXdkGLIAJ3k84c8WZoc8GRgeI1PWifTTCwvCL+ycRr7\n\
7wtCjmRZP7KxRiTt8G4S4ga9AN/bwuZuI8ExAr7nOmsesaiGIewgSIqmLCHqp+bZ\n\
INL71WPGhXYSw3gzpoNqIcO0jf3rg+KAwmDkjA+TX97yYckm4I7vtNQrXhhyUgMc\n\
tmYspvDoBt1ZnK4FBvxVu9qn4KVA8geLOrg+qZANoMzIUnGCm9D6sUBsNFiCxN5H\n\
mhfovzipAgMBAAECggEAIcMGMaSmx0Bk09kIIOK1z5oRxfdPmXCJ7yPcPhbU8QXF\n\
c6iBupnfTtjg1SXriBQzETQtDZnlwKWbY5bPKq0F8BYk2mzbajnICL9kLCN4OrcD\n\
wGj6TBb/u7Bkx+k/ShQs6H7FJqgbBpvbWA+KTZ0PIDfVCC5P4N89+kfY55mxQIZz\n\
tn4VPjLJTsRST0mnuqTxOLF0ATKzU07u8xBP3NVCqASFGfRQPrhbyhRoSE5Pm6he\n\
5dUdz//RhtIedpMjMJO/iVIpLXLsGWRTmmwWupO6BtGBuwSWWPCDkI6qwSCf7Noy\n\
4CAPQ2DQGUF58gFwtp4BKpd4tx9h1EnkU4X6ubTdEQKBgQD7Vi8Q2Ay+u8iwUuRc\n\
6LBUH9AS9rOJY8iqhONOJDQhaAv8E7z9+IiGMgPFKrWFp/v2+6LbL5JaEeLlJA/v\n\
ZPazGyNgqPKlTSIpCrWlUx+V+gOyNH76hxvUi9fcbRbstijmW1PX/WqnPBriONw+\n\
mj3jNcAhQ6g8CUFlAIv7qU525QKBgQDxrxAtOU6F4ORRFcLx8e0GyPgo/K1fq9ge\n\
RJzBdGk91Ob7zw2qKcAO4w/montvA1GaQNjQGgNNmxWNX+oVnO+X4c8R15OSNqmw\n\
UjJbXWadUuNHyqY+JJcNg5q2Ve285OIekfdPSmKx9x44bnrfpBJGJuCBETWipZxi\n\
XsssQvc6dQKBgDNjs8vl4PU+wBINYNP+X89Tkd/OwXbeCDGVakSX8nDCLXElOAdV\n\
wdRudYbi7KqfZk1htjLKz0nLTnE7pmZ0ZlzIt7sT0EksNEfgALQFAvhPXmIZibz/\n\
0xjqXwCa7Y0I0eQH2GTZU+1NxNFsftvt/alvXBFxG/zqh4x3SCf0vi5hAoGBAOWB\n\
xE/d2raB4O8rRivx/I9z600o3g87JglgSKfhP0uLUSoQ7r4H1a2NbH0tESBTu3tL\n\
V1kPStG4kxfk3GtX06KcucIMwMOZizy4Yb+ni5mcq95yD7p1jsgzkIjUQuYdSKmV\n\
HZA7aEvuCtG2AJM9wGjD5HBMgm2I7V/w+ul2UkY9AoGBAOmBa+kZdDujFQMeLr5r\n\
W563s7vPXBr8Jtsv1K9OAugVO1mykJMO+yKJf5Ggm5eDmQK8Ze7lt4CMzkh4ojhA\n\
Mdf5TvSMGeQrOlod223fSU+eW1RyF57JorBHY4TBGpgTC3KwAlri9IJxUI0DFy3r\n\
uPPiyZuqLoKYBEWIMr+6O7MK\n\
-----END PRIVATE KEY-----\n";

    const JWK_N: &str = "7UgCGtFCS-mZt2kB5FmuTuWZtfYa3qX20mUQt7DMd3DDSRs-fHYZxU93x012vAvWFxg_hTN08Ag8pjTXKcLoAk7tC6jcCep74gFl3ZBiyACd5POHPFmaHPBkYHiNT1on00wsLwi_snEa--8LQo5kWT-ysUYk7fBuEuIGvQDf28LmbiPBMQK-5zprHrGohiHsIEiKpiwh6qfm2SDS-9VjxoV2EsN4M6aDaiHDtI3964PigMJg5IwPk1_e8mHJJuCO77TUK14YclIDHLZmLKbw6AbdWZyuBQb8Vbvap-ClQPIHizq4PqmQDaDMyFJxgpvQ-rFAbDRYgsTeR5oX6L84qQ";

    fn jwks() -> JwkSet {
        let json = serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "use": "sig",
                "alg": "RS256",
                "kid": "test",
                "n": JWK_N,
                "e": "AQAB",
            }]
        });
        serde_json::from_value(json).unwrap()
    }

    #[derive(Serialize)]
    struct TestClaims {
        sub: String,
        email: String,
        email_verified: bool,
        nonce: String,
        iss: String,
        aud: String,
        exp: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        tid: Option<String>,
    }

    fn sign(claims: &TestClaims) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("test".into());
        let key = EncodingKey::from_rsa_pem(PRIVATE_PEM.as_bytes()).unwrap();
        encode(&header, claims, &key).unwrap()
    }

    fn claims() -> TestClaims {
        TestClaims {
            sub: "user-123".into(),
            email: "alice@example.com".into(),
            email_verified: true,
            nonce: NONCE.into(),
            iss: ISSUER.into(),
            aud: CLIENT_ID.into(),
            exp: chrono::Utc::now().timestamp() + 3600,
            tid: None,
        }
    }

    #[test]
    fn accepts_valid_token() {
        let token = sign(&claims());
        let out = verify_with_jwks(&jwks(), &token, ISSUER, CLIENT_ID, NONCE).unwrap();
        assert_eq!(out.subject, "user-123");
        assert_eq!(out.email.as_deref(), Some("alice@example.com"));
        assert_eq!(out.email_verified, Some(true));
    }

    #[test]
    fn rejects_wrong_audience() {
        let token = sign(&claims());
        let err = verify_with_jwks(&jwks(), &token, ISSUER, "other-client", NONCE);
        assert!(matches!(err, Err(IdTokenError::Invalid(_))));
    }

    #[test]
    fn rejects_wrong_issuer() {
        let token = sign(&claims());
        let err = verify_with_jwks(&jwks(), &token, "https://evil.example.com", CLIENT_ID, NONCE);
        assert!(matches!(err, Err(IdTokenError::Invalid(_))));
    }

    #[test]
    fn rejects_nonce_mismatch() {
        let token = sign(&claims());
        let err = verify_with_jwks(&jwks(), &token, ISSUER, CLIENT_ID, "other-nonce");
        assert!(matches!(err, Err(IdTokenError::NonceMismatch)));
    }

    #[test]
    fn rejects_expired_token() {
        let mut c = claims();
        c.exp = chrono::Utc::now().timestamp() - 3600;
        let token = sign(&c);
        let err = verify_with_jwks(&jwks(), &token, ISSUER, CLIENT_ID, NONCE);
        assert!(matches!(err, Err(IdTokenError::Invalid(_))));
    }

    #[test]
    fn rejects_tampered_signature() {
        let token = sign(&claims());
        // Flip the last character of the signature segment.
        let mut chars: Vec<char> = token.chars().collect();
        let last = chars.len() - 1;
        chars[last] = if chars[last] == 'A' { 'B' } else { 'A' };
        let tampered: String = chars.into_iter().collect();
        let err = verify_with_jwks(&jwks(), &tampered, ISSUER, CLIENT_ID, NONCE);
        assert!(err.is_err());
    }

    #[test]
    fn accepts_entra_tenantid_template_issuer() {
        // Configured issuer carries the `{tenantid}` placeholder (Entra
        // multi-tenant); the token's iss has the concrete tenant + matching tid.
        let tenant = "11111111-2222-3333-4444-555555555555";
        let mut c = claims();
        c.iss = format!("https://login.microsoftonline.com/{tenant}/v2.0");
        c.tid = Some(tenant.into());
        let token = sign(&c);
        let template = "https://login.microsoftonline.com/{tenantid}/v2.0";
        let out = verify_with_jwks(&jwks(), &token, template, CLIENT_ID, NONCE).unwrap();
        assert_eq!(out.subject, "user-123");
    }

    #[test]
    fn rejects_entra_template_when_tid_mismatches_iss() {
        // A token whose iss tenant doesn't match its own tid must be rejected.
        let mut c = claims();
        c.iss = "https://login.microsoftonline.com/REAL-TENANT/v2.0".into();
        c.tid = Some("DIFFERENT-TENANT".into());
        let token = sign(&c);
        let template = "https://login.microsoftonline.com/{tenantid}/v2.0";
        let err = verify_with_jwks(&jwks(), &token, template, CLIENT_ID, NONCE);
        assert!(matches!(err, Err(IdTokenError::Invalid(_))));
    }

    #[test]
    fn email_verified_parsing() {
        assert_eq!(parse_bool(Some(&serde_json::json!("true"))), Some(true));
        assert_eq!(parse_bool(Some(&serde_json::json!(true))), Some(true));
        assert_eq!(parse_bool(Some(&serde_json::json!("false"))), Some(false));
        assert_eq!(parse_bool(Some(&serde_json::json!(false))), Some(false));
        // Absent / null / unrecognized → None (claim not asserted).
        assert_eq!(parse_bool(None), None);
        assert_eq!(parse_bool(Some(&serde_json::json!(null))), None);
    }
}
