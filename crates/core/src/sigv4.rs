//! AWS Signature Version 4 primitives, shared by every hand-rolled AWS client
//! in this crate ([`crate::storage::s3`] presigns query strings,
//! [`crate::email::ses`] signs a request header).
//!
//! Only the parts that are identical across services live here: HMAC, the
//! hex/percent encodings SigV4 defines, and the four-step signing-key
//! derivation, parameterised by the service name. Canonicalisation stays with
//! each caller, because a presigned URL and a header-signed request
//! canonicalise differently, and each caller pins its own shape against a
//! published example.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

pub const ALGORITHM: &str = "AWS4-HMAC-SHA256";

type HmacSha256 = Hmac<Sha256>;

pub fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex(&hasher.finalize())
}

/// RFC 3986 percent-encoding as SigV4 defines it: unreserved characters pass,
/// everything else becomes uppercase `%XX`. `keep_slash` is set for path
/// segments (where `/` is a separator) and clear for query components.
///
/// Hand-rolled because the `url`/`percent-encoding` crates aren't dependencies
/// and neither offers exactly this set without configuration anyway.
pub fn uri_encode(input: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        let c = *byte as char;
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            '/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The SigV4 four-step key derivation: date → region → service → `aws4_request`.
pub fn signing_key(secret: &str, date_stamp: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date_stamp.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

/// Derive the signing key, then sign `string_to_sign` with it.
pub fn sign(
    secret: &str,
    date_stamp: &str,
    region: &str,
    service: &str,
    string_to_sign: &[u8],
) -> Vec<u8> {
    hmac_sha256(
        &signing_key(secret, date_stamp, region, service),
        string_to_sign,
    )
}

/// `{date}/{region}/{service}/aws4_request`.
pub fn credential_scope(date_stamp: &str, region: &str, service: &str) -> String {
    format!("{date_stamp}/{region}/{service}/aws4_request")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AWS's published derivation example ("Deriving the signing key",
    /// `iam`, 2012-02-15). Pins that the service name is really threaded
    /// through rather than defaulting to `s3`.
    #[test]
    fn signing_key_matches_published_iam_example() {
        let k = signing_key(
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "20120215",
            "us-east-1",
            "iam",
        );
        assert_eq!(
            hex(&k),
            "f4780e2d9f65fa895f9c67b32ce1baf0b0d8a43505a000a1a9e090d414db404d"
        );
    }

    #[test]
    fn credential_scope_has_the_documented_shape() {
        assert_eq!(
            credential_scope("20150830", "us-east-1", "service"),
            "20150830/us-east-1/service/aws4_request"
        );
    }
}
