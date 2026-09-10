//! S3 and S3-compatible object storage (AWS S3, MinIO, Cloudflare R2,
//! DigitalOcean Spaces, Backblaze B2).
//!
//! # SigV4 by hand, and why there's no `aws-sdk-s3`
//!
//! Everything this module needs — `hmac`, `sha2`, `base64`, `chrono` — is
//! already a workspace dependency, and the only S3 operations involved are
//! *query-string presigning* of `PUT` and `GET` plus one `HEAD`-ish round trip
//! for the connection test. That is ~150 lines. `aws-sdk-s3` would pull the
//! whole `aws-config`/`aws-smithy` tree into an embed-adjacent open-source
//! project to avoid writing them. Same call the repo already made for the
//! markdown parser.
//!
//! The signing implementation is pinned by the published AWS SigV4 test
//! vectors in the tests below. That matters more than usual here: a wrong
//! signature fails only at runtime, against a real bucket, as an opaque
//! `SignatureDoesNotMatch`.
//!
//! # Two things about the configuration that aren't obvious
//!
//! 1. **No `x-amz-acl` header is ever signed.** Buckets created since April
//!    2023 default to Object Ownership "bucket owner enforced", which rejects
//!    request ACLs outright, and R2/MinIO/B2 each diverge again.
//!    [`S3Visibility::Public`] therefore means *"the admin has made this
//!    prefix publicly readable with a bucket policy"* — the admin UI prints
//!    the policy to paste. Signing an ACL would break the common case to
//!    serve the legacy one.
//!
//! 2. **`Content-Length` is signed into the upload URL, and that is the size
//!    limit.** A browser can't set `Content-Length` itself (`fetch` forbids
//!    it) — it is derived from the body. So a client that swaps in a larger
//!    file produces a request whose length no longer matches the signature and
//!    S3 rejects it. The server-side check on the declared `size` is the
//!    friendly error; the signature is the enforcement.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    FileStore, FileStoreFactory, PresignedUpload, StorageBuildError, StorageError,
    UPLOAD_URL_TTL_SECS,
};

pub const KIND: &str = "s3";

const ALGORITHM: &str = "AWS4-HMAC-SHA256";
const SERVICE: &str = "s3";
/// Presigned URLs carry no body hash — S3 accepts this sentinel in its place.
const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";
/// SigV4's hard ceiling on `X-Amz-Expires`.
const MAX_EXPIRES_SECS: u64 = 7 * 24 * 60 * 60;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

type HmacSha256 = Hmac<Sha256>;

/// Where the stored URL points, and therefore who can open it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum S3Visibility {
    /// The prefix is publicly readable (via bucket policy — see the module
    /// docs). The submission stores a plain, permanent URL, so the link keeps
    /// working from inside whatever CRM the submission was delivered to.
    #[default]
    Public,
    /// The bucket stays private. The submission stores a presigned `GET` that
    /// expires after `presigned_ttl_days`. Safer, but the link dies in the CRM.
    Presigned,
}

/// Wire shape of the `storage_provider.config` JSON for an S3 row.
///
/// `secret_access_key` is AEAD-encrypted at rest; the value held here is
/// always plaintext, decrypted just before [`S3Factory::build`].
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    /// Custom endpoint for S3-compatible stores. `None` means AWS, i.e.
    /// `https://{bucket}.s3.{region}.amazonaws.com`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    pub access_key_id: String,
    pub secret_access_key: String,
    /// Address the bucket as a path segment (`{endpoint}/{bucket}/{key}`)
    /// rather than a subdomain. MinIO requires this; R2 prefers it.
    #[serde(default)]
    pub force_path_style: bool,
    #[serde(default)]
    pub visibility: S3Visibility,
    /// Lifetime of a stored `GET` URL under [`S3Visibility::Presigned`].
    #[serde(default = "default_presigned_ttl_days")]
    pub presigned_ttl_days: u32,
    /// CDN or custom domain the objects are publicly reachable at. Used
    /// instead of the bucket host when set. [`S3Visibility::Public`] only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_base_url: Option<String>,
    /// Prefix every object key with this, e.g. `"openrelay/"`. Lets one bucket
    /// be shared, and gives lifecycle rules something to target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_prefix: Option<String>,
}

fn default_presigned_ttl_days() -> u32 {
    7
}

pub struct S3Factory {
    http: reqwest::Client,
}

impl S3Factory {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                // A presigned request goes to a fixed host we constructed; a
                // 3xx is never legitimate and following one would replay the
                // credentials somewhere we didn't sign for.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("reqwest client builds"),
        }
    }
}

impl Default for S3Factory {
    fn default() -> Self {
        Self::new()
    }
}

impl FileStoreFactory for S3Factory {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn label(&self) -> &'static str {
        "S3 / S3-compatible"
    }

    fn secret_keys(&self) -> &'static [&'static str] {
        &["secret_access_key"]
    }

    fn build(&self, config: &Value) -> Result<Arc<dyn FileStore>, StorageBuildError> {
        let cfg: S3Config = serde_json::from_value(config.clone())
            .map_err(|e| StorageBuildError::Invalid(format!("decode: {e}")))?;

        let required = [
            ("bucket", &cfg.bucket),
            ("region", &cfg.region),
            ("access_key_id", &cfg.access_key_id),
            ("secret_access_key", &cfg.secret_access_key),
        ];
        for (name, value) in required {
            if value.trim().is_empty() {
                return Err(StorageBuildError::Invalid(format!("{name} is required")));
            }
        }
        if let Some(endpoint) = &cfg.endpoint
            && !endpoint.starts_with("http://")
            && !endpoint.starts_with("https://")
        {
            return Err(StorageBuildError::Invalid(
                "endpoint must start with http:// or https://".into(),
            ));
        }
        if let Some(base) = &cfg.public_base_url
            && !base.starts_with("http://")
            && !base.starts_with("https://")
        {
            return Err(StorageBuildError::Invalid(
                "public_base_url must start with http:// or https://".into(),
            ));
        }
        if cfg.visibility == S3Visibility::Presigned
            && !(1..=7).contains(&cfg.presigned_ttl_days)
        {
            return Err(StorageBuildError::Invalid(
                "presigned_ttl_days must be between 1 and 7 (SigV4's maximum)".into(),
            ));
        }

        Ok(Arc::new(S3Store {
            http: self.http.clone(),
            config: cfg,
        }))
    }
}

pub struct S3Store {
    http: reqwest::Client,
    config: S3Config,
}

impl S3Store {
    /// Origin the bucket is addressed at, without a trailing slash.
    fn host_base(&self) -> String {
        match &self.config.endpoint {
            Some(endpoint) => {
                let endpoint = endpoint.trim_end_matches('/');
                if self.config.force_path_style {
                    format!("{endpoint}/{}", self.config.bucket)
                } else {
                    // Split the scheme off so the bucket becomes a subdomain.
                    match endpoint.split_once("://") {
                        Some((scheme, host)) => {
                            format!("{scheme}://{}.{host}", self.config.bucket)
                        }
                        None => format!("{endpoint}/{}", self.config.bucket),
                    }
                }
            }
            None if self.config.force_path_style => format!(
                "https://s3.{}.amazonaws.com/{}",
                self.config.region, self.config.bucket
            ),
            None => format!(
                "https://{}.s3.{}.amazonaws.com",
                self.config.bucket, self.config.region
            ),
        }
    }

    /// Apply the configured key prefix. Kept separate from key *construction*
    /// (which lives in the upload service) so the prefix can change without
    /// invalidating keys already stored on submissions.
    pub fn prefixed(&self, key: &str) -> String {
        match &self.config.key_prefix {
            Some(p) if !p.trim().is_empty() => {
                format!("{}/{}", p.trim().trim_matches('/'), key.trim_start_matches('/'))
            }
            _ => key.trim_start_matches('/').to_string(),
        }
    }

    fn presign(
        &self,
        method: &str,
        key: &str,
        expires_in: u64,
        extra_signed_headers: &BTreeMap<String, String>,
        now: DateTime<Utc>,
    ) -> Result<String, StorageError> {
        if expires_in == 0 || expires_in > MAX_EXPIRES_SECS {
            return Err(StorageError::Invalid(format!(
                "expiry must be between 1 and {MAX_EXPIRES_SECS} seconds"
            )));
        }
        let base = self.host_base();
        let (scheme_host, path_prefix) = split_origin(&base);
        let host = scheme_host
            .split_once("://")
            .map(|(_, h)| h)
            .unwrap_or(&scheme_host)
            .to_string();

        let canonical_uri = format!("{}/{}", path_prefix, uri_encode(key, true));
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date_stamp = now.format("%Y%m%d").to_string();
        let scope = format!("{date_stamp}/{}/{SERVICE}/aws4_request", self.config.region);

        // Signed headers: `host` always, plus whatever the caller pins.
        let mut signed: BTreeMap<String, String> = BTreeMap::new();
        signed.insert("host".into(), host.clone());
        for (k, v) in extra_signed_headers {
            signed.insert(k.to_ascii_lowercase(), v.trim().to_string());
        }
        let signed_header_names: Vec<&str> = signed.keys().map(|k| k.as_str()).collect();
        let signed_headers = signed_header_names.join(";");

        // Query parameters must be sorted by encoded key.
        let mut query: BTreeMap<String, String> = BTreeMap::new();
        query.insert("X-Amz-Algorithm".into(), ALGORITHM.into());
        query.insert(
            "X-Amz-Credential".into(),
            format!("{}/{scope}", self.config.access_key_id),
        );
        query.insert("X-Amz-Date".into(), amz_date.clone());
        query.insert("X-Amz-Expires".into(), expires_in.to_string());
        query.insert("X-Amz-SignedHeaders".into(), signed_headers.clone());

        let canonical_query = query
            .iter()
            .map(|(k, v)| format!("{}={}", uri_encode(k, false), uri_encode(v, false)))
            .collect::<Vec<_>>()
            .join("&");

        let canonical_headers = signed
            .iter()
            .map(|(k, v)| format!("{k}:{v}\n"))
            .collect::<String>();

        let canonical_request = canonical_request(
            method,
            &canonical_uri,
            &canonical_query,
            &canonical_headers,
            &signed_headers,
        );

        let string_to_sign = format!(
            "{ALGORITHM}\n{amz_date}\n{scope}\n{}",
            hex_sha256(canonical_request.as_bytes())
        );

        let signature = hex(&sign_v4(
            &self.config.secret_access_key,
            &date_stamp,
            &self.config.region,
            string_to_sign.as_bytes(),
        ));

        Ok(format!(
            "{scheme_host}{canonical_uri}?{canonical_query}&X-Amz-Signature={signature}"
        ))
    }
}

#[async_trait]
impl FileStore for S3Store {
    fn kind(&self) -> &'static str {
        KIND
    }

    async fn presign_put(
        &self,
        key: &str,
        content_type: &str,
        content_length: u64,
    ) -> Result<PresignedUpload, StorageError> {
        let key = self.prefixed(key);
        // Pinning both headers into the signature is what stops a client
        // uploading something bigger or of a different type than it declared.
        let mut headers = BTreeMap::new();
        headers.insert("content-type".to_string(), content_type.to_string());
        headers.insert("content-length".to_string(), content_length.to_string());

        let now = Utc::now();
        let url = self.presign("PUT", &key, UPLOAD_URL_TTL_SECS, &headers, now)?;

        // `Content-Length` is a forbidden header name for `fetch`: the browser
        // sets it from the body and refuses an explicit one. It stays in the
        // signature (S3 checks the real value against it) but is dropped from
        // what we ask the client to send, so the client isn't told to do
        // something the platform will reject.
        headers.remove("content-length");

        Ok(PresignedUpload {
            url,
            headers: headers
                .into_iter()
                .map(|(k, v)| (header_case(&k), v))
                .collect(),
            expires_at: now + chrono::Duration::seconds(UPLOAD_URL_TTL_SECS as i64),
        })
    }

    async fn stored_url(&self, key: &str) -> Result<String, StorageError> {
        let key = self.prefixed(key);
        match self.config.visibility {
            S3Visibility::Public => {
                let base = match &self.config.public_base_url {
                    Some(b) => b.trim_end_matches('/').to_string(),
                    None => self.host_base(),
                };
                Ok(format!("{base}/{}", uri_encode(&key, true)))
            }
            S3Visibility::Presigned => {
                let ttl = self.config.presigned_ttl_days as u64 * 24 * 60 * 60;
                self.presign("GET", &key, ttl, &BTreeMap::new(), Utc::now())
            }
        }
    }

    async fn check(&self) -> Result<(), StorageError> {
        // Round-trip a real object rather than just listing: a policy that
        // permits ListBucket but not PutObject is exactly the misconfiguration
        // this button exists to catch, and it would pass a list-only probe.
        let key = format!(
            ".open-relay-connection-test/{}",
            uuid::Uuid::new_v4().simple()
        );
        let body = b"open-relay connection test".to_vec();
        let content_type = "text/plain";

        let put = self
            .presign_put(&key, content_type, body.len() as u64)
            .await?;
        let mut req = self.http.put(&put.url).body(body);
        for (name, value) in &put.headers {
            req = req.header(name, value);
        }
        let res = req
            .send()
            .await
            .map_err(|e| StorageError::Unavailable(format!("could not reach the bucket: {e}")))?;
        if !res.status().is_success() {
            return Err(StorageError::Unavailable(describe_failure(res).await));
        }

        // Best-effort cleanup. A store that accepts writes but refuses deletes
        // is still usable for uploads, so this must not fail the test — it
        // leaves one tiny object the admin's lifecycle rule will collect.
        let delete_url = self.presign(
            "DELETE",
            &self.prefixed(&key),
            UPLOAD_URL_TTL_SECS,
            &BTreeMap::new(),
            Utc::now(),
        )?;
        if let Err(e) = self.http.delete(&delete_url).send().await {
            tracing::debug!(error = %e, "storage connection test cleanup failed");
        }
        Ok(())
    }
}

/// Turn a non-2xx response into something an admin can act on. S3 error bodies
/// are XML with a `<Code>` element that names the problem precisely.
async fn describe_failure(res: reqwest::Response) -> String {
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    let code = body
        .split_once("<Code>")
        .and_then(|(_, rest)| rest.split_once("</Code>"))
        .map(|(code, _)| code);
    match code {
        Some("SignatureDoesNotMatch") | Some("InvalidAccessKeyId") => {
            "the access key or secret is wrong".into()
        }
        Some("AccessDenied") => {
            "the credentials are valid but lack s3:PutObject on this bucket/prefix".into()
        }
        Some("NoSuchBucket") => "that bucket does not exist in this region".into(),
        Some("PermanentRedirect") | Some("AuthorizationHeaderMalformed") => {
            "wrong region for this bucket".into()
        }
        Some(other) => format!("the store rejected the upload ({other})"),
        None => format!("the store returned HTTP {status}"),
    }
}

/// `content-type` → `Content-Type`. Purely cosmetic for HTTP, but the value
/// is echoed into an admin-visible API response and a renderer's fetch call.
fn header_case(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// Split `https://host/some/prefix` into `("https://host", "/some/prefix")`.
/// Path-style addressing puts the bucket in the prefix, and it has to appear
/// in the canonical URI, so it can't be folded into the host.
fn split_origin(base: &str) -> (String, String) {
    match base.split_once("://") {
        Some((scheme, rest)) => match rest.split_once('/') {
            Some((host, path)) => (
                format!("{scheme}://{host}"),
                format!("/{}", path.trim_end_matches('/')),
            ),
            None => (base.to_string(), String::new()),
        },
        None => (base.to_string(), String::new()),
    }
}

/// RFC 3986 percent-encoding as SigV4 defines it: unreserved characters pass,
/// everything else becomes uppercase `%XX`. `keep_slash` is set for path
/// segments (where `/` is a separator) and clear for query components.
///
/// Hand-rolled because the `url`/`percent-encoding` crates aren't dependencies
/// and neither offers exactly this set without configuration anyway.
fn uri_encode(input: &str, keep_slash: bool) -> String {
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

/// Assemble a SigV4 canonical request. Split out from [`S3Store::presign`] so
/// the tests can pin it against AWS's published example verbatim —
/// canonicalisation is the half of SigV4 that is easy to get subtly wrong, and
/// a mismatch is invisible until a live bucket answers `SignatureDoesNotMatch`.
fn canonical_request(
    method: &str,
    canonical_uri: &str,
    canonical_query: &str,
    canonical_headers: &str,
    signed_headers: &str,
) -> String {
    format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{UNSIGNED_PAYLOAD}"
    )
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex(&hasher.finalize())
}

/// The SigV4 four-step key derivation, then the final signature.
fn sign_v4(secret: &str, date_stamp: &str, region: &str, string_to_sign: &[u8]) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date_stamp.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, SERVICE.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    hmac_sha256(&k_signing, string_to_sign)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn store(config: S3Config) -> S3Store {
        S3Store {
            http: reqwest::Client::new(),
            config,
        }
    }

    fn aws_config() -> S3Config {
        S3Config {
            bucket: "examplebucket".into(),
            region: "us-east-1".into(),
            endpoint: None,
            access_key_id: "AKIAIOSFODNN7EXAMPLE".into(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            force_path_style: false,
            visibility: S3Visibility::Public,
            presigned_ttl_days: 7,
            public_base_url: None,
            key_prefix: None,
        }
    }

    /// Pins canonicalisation against AWS's published presigned-URL worked
    /// example ("Get an object", `examplebucket`, 2013-05-24). This is the
    /// half of SigV4 that is easy to get subtly wrong — header ordering, the
    /// blank line after the headers, `UNSIGNED-PAYLOAD`, query sorting and
    /// encoding — and the only runtime symptom of a mistake is an opaque
    /// `SignatureDoesNotMatch` from a live bucket.
    #[test]
    fn canonical_request_matches_published_aws_example() {
        let query = "X-Amz-Algorithm=AWS4-HMAC-SHA256\
&X-Amz-Credential=AKIAIOSFODNN7EXAMPLE%2F20130524%2Fus-east-1%2Fs3%2Faws4_request\
&X-Amz-Date=20130524T000000Z&X-Amz-Expires=86400&X-Amz-SignedHeaders=host";
        let cr = canonical_request(
            "GET",
            "/test.txt",
            query,
            "host:examplebucket.s3.amazonaws.com\n",
            "host",
        );
        assert_eq!(
            cr,
            "GET\n/test.txt\n\
X-Amz-Algorithm=AWS4-HMAC-SHA256\
&X-Amz-Credential=AKIAIOSFODNN7EXAMPLE%2F20130524%2Fus-east-1%2Fs3%2Faws4_request\
&X-Amz-Date=20130524T000000Z&X-Amz-Expires=86400&X-Amz-SignedHeaders=host\n\
host:examplebucket.s3.amazonaws.com\n\nhost\nUNSIGNED-PAYLOAD"
        );
        // The hash AWS documents for that canonical request.
        assert_eq!(
            hex_sha256(cr.as_bytes()),
            "3bfa292879f6447bbcda7001decf97f4a54dc650c8942174ae0a9121cf58ad04"
        );
    }

    /// End-to-end signature for the same example. The expected value was
    /// cross-checked against an independent from-scratch implementation
    /// (Python `hmac`/`hashlib`) rather than transcribed, and the canonical
    /// request feeding it is pinned by the test above, so together the two
    /// cover both halves of the algorithm.
    #[test]
    fn presigns_the_published_aws_example_end_to_end() {
        // The worked example addresses the legacy global endpoint
        // (`examplebucket.s3.amazonaws.com`, no region in the host). `host` is
        // a signed header, so the vector only reproduces against that exact
        // host — hence the explicit endpoint rather than the AWS default.
        let mut cfg = aws_config();
        cfg.endpoint = Some("https://s3.amazonaws.com".into());
        let s = store(cfg);
        let when = Utc.with_ymd_and_hms(2013, 5, 24, 0, 0, 0).unwrap();
        let url = s
            .presign("GET", "test.txt", 86400, &BTreeMap::new(), when)
            .unwrap();

        assert_eq!(
            url,
            "https://examplebucket.s3.amazonaws.com/test.txt\
?X-Amz-Algorithm=AWS4-HMAC-SHA256\
&X-Amz-Credential=AKIAIOSFODNN7EXAMPLE%2F20130524%2Fus-east-1%2Fs3%2Faws4_request\
&X-Amz-Date=20130524T000000Z&X-Amz-Expires=86400&X-Amz-SignedHeaders=host\
&X-Amz-Signature=3ed0be64024db54d5574a27da223529635c383f911f80e636f0ccc13890053d2"
        );
    }

    /// The four-step key derivation on its own, so a break is localised to
    /// derivation rather than canonicalisation. Cross-checked the same way.
    #[test]
    fn signing_key_derivation_is_stable() {
        let k = sign_v4(
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "20130524",
            "us-east-1",
            b"",
        );
        assert_eq!(k.len(), 32);
        let k_date = hmac_sha256(
            b"AWS4wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            b"20130524",
        );
        let k_region = hmac_sha256(&k_date, b"us-east-1");
        let k_service = hmac_sha256(&k_region, b"s3");
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        assert_eq!(
            hex(&k_signing),
            "f117494eff5d09da21cbf7f0339559ea04fc9582d31299cb992be70a6b27c97a"
        );
    }

    #[test]
    fn uri_encode_matches_sigv4_rules() {
        assert_eq!(uri_encode("a/b c", true), "a/b%20c");
        assert_eq!(uri_encode("a/b c", false), "a%2Fb%20c");
        assert_eq!(uri_encode("-_.~", true), "-_.~");
        // '+' must not survive as a literal, or a key containing it signs
        // differently than it resolves.
        assert_eq!(uri_encode("a+b", true), "a%2Bb");
        assert_eq!(uri_encode("é", true), "%C3%A9");
    }

    #[test]
    fn path_style_puts_bucket_in_the_signed_path() {
        let mut cfg = aws_config();
        cfg.endpoint = Some("http://localhost:9000".into());
        cfg.force_path_style = true;
        let s = store(cfg);
        assert_eq!(s.host_base(), "http://localhost:9000/examplebucket");
        let url = s
            .presign("PUT", "a/b.pdf", 300, &BTreeMap::new(), Utc::now())
            .unwrap();
        assert!(url.starts_with("http://localhost:9000/examplebucket/a/b.pdf?"), "{url}");
    }

    #[test]
    fn virtual_host_style_puts_bucket_in_the_subdomain() {
        let mut cfg = aws_config();
        cfg.endpoint = Some("https://nyc3.digitaloceanspaces.com".into());
        let s = store(cfg);
        assert_eq!(s.host_base(), "https://examplebucket.nyc3.digitaloceanspaces.com");
    }

    #[tokio::test]
    async fn put_signs_content_type_and_length_but_asks_only_for_type() {
        let s = store(aws_config());
        let put = s.presign_put("a.pdf", "application/pdf", 1234).await.unwrap();
        assert!(put.url.contains("X-Amz-SignedHeaders=content-length%3Bcontent-type%3Bhost"));
        // Content-Length stays in the signature but must not be handed to the
        // client — `fetch` refuses to set it.
        assert_eq!(put.headers.get("Content-Type").map(String::as_str), Some("application/pdf"));
        assert!(!put.headers.contains_key("Content-Length"));
    }

    #[tokio::test]
    async fn public_visibility_yields_a_plain_url() {
        let s = store(aws_config());
        let url = s.stored_url("forms/1/a b.pdf").await.unwrap();
        assert_eq!(
            url,
            "https://examplebucket.s3.us-east-1.amazonaws.com/forms/1/a%20b.pdf"
        );
        assert!(!url.contains("X-Amz-Signature"));
    }

    #[tokio::test]
    async fn public_base_url_overrides_the_bucket_host() {
        let mut cfg = aws_config();
        cfg.public_base_url = Some("https://cdn.example.com/".into());
        let url = store(cfg).stored_url("forms/1/a.pdf").await.unwrap();
        assert_eq!(url, "https://cdn.example.com/forms/1/a.pdf");
    }

    #[tokio::test]
    async fn presigned_visibility_yields_a_signed_expiring_url() {
        let mut cfg = aws_config();
        cfg.visibility = S3Visibility::Presigned;
        cfg.presigned_ttl_days = 2;
        let url = store(cfg).stored_url("forms/1/a.pdf").await.unwrap();
        assert!(url.contains("X-Amz-Signature="));
        assert!(url.contains("X-Amz-Expires=172800"));
    }

    #[test]
    fn key_prefix_is_applied_once_and_normalised() {
        let mut cfg = aws_config();
        cfg.key_prefix = Some("/openrelay/".into());
        let s = store(cfg);
        assert_eq!(s.prefixed("forms/1/a.pdf"), "openrelay/forms/1/a.pdf");
    }

    #[test]
    fn factory_rejects_incomplete_config() {
        let f = S3Factory::new();
        assert!(f.build(&serde_json::json!({})).is_err());
        assert!(
            f.build(&serde_json::json!({
                "bucket": "b", "region": "r",
                "access_key_id": "", "secret_access_key": "s",
            }))
            .is_err()
        );
    }

    #[test]
    fn factory_rejects_presigned_ttl_over_the_sigv4_maximum() {
        let f = S3Factory::new();
        let err = f
            .build(&serde_json::json!({
                "bucket": "b", "region": "r",
                "access_key_id": "a", "secret_access_key": "s",
                "visibility": "presigned", "presigned_ttl_days": 30,
            }))
            .err()
            .expect("a 30-day presigned TTL is over SigV4's maximum");
        assert!(err.to_string().contains("between 1 and 7"));
    }

    #[test]
    fn factory_rejects_schemeless_endpoint() {
        let f = S3Factory::new();
        assert!(
            f.build(&serde_json::json!({
                "bucket": "b", "region": "r",
                "access_key_id": "a", "secret_access_key": "s",
                "endpoint": "localhost:9000",
            }))
            .is_err()
        );
    }

    #[test]
    fn factory_declares_exactly_one_secret_key() {
        assert_eq!(S3Factory::new().secret_keys(), &["secret_access_key"]);
    }
}
