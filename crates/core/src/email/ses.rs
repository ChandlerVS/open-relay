//! AWS Simple Email Service (SES v2) sender.
//!
//! # SigV4 by hand, like `storage/s3.rs`
//!
//! The only call needed is `SendEmail` (`POST /v2/email/outbound-emails`), so
//! this signs one JSON request with the shared [`crate::sigv4`] primitives
//! rather than pulling `aws-sdk-sesv2` and the whole smithy tree. Unlike S3's
//! presigned URLs, this request is signed in the `Authorization` header, so it
//! has its own canonicalisation — pinned below against AWS's published
//! `post-x-www-form-urlencoded` test-suite vector.
//!
//! # Errors an admin will actually hit
//!
//! A fresh SES account is in the *sandbox*: it can only send **from** a
//! verified identity **to** verified recipients. Both surface as
//! `MessageRejected` (HTTP 400) and are permanent — retrying won't verify the
//! address — so they map to [`EmailError::Permanent`] with a message that
//! names the fix. Throttling, quota and 5xx map to [`EmailError::Transient`].

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{EmailBuildError, EmailError, EmailSender, EmailSenderFactory, OutgoingEmail};
use crate::sigv4::{self, ALGORITHM, hex, hex_sha256};

pub const KIND: &str = "ses";

const SERVICE: &str = "ses";
const SEND_PATH: &str = "/v2/email/outbound-emails";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_FROM_NAME_LEN: usize = 100;
/// SES caps a subject well above this; keeping it short keeps inbox previews
/// readable and bounds what an admin-chosen form name can contribute.
const MAX_SUBJECT_LEN: usize = 200;

/// Wire shape of the `email_provider.config` JSON for an SES row.
///
/// `secret_access_key` is AEAD-encrypted at rest; the value held here is
/// always plaintext, decrypted just before [`SesFactory::build`].
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SesConfig {
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    /// Verified SES identity (address, or an address on a verified domain).
    pub from_address: String,
    /// Display name shown beside `from_address`, e.g. "OpenRelay".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_name: Option<String>,
    /// SES configuration set for event publishing / dedicated IPs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_set: Option<String>,
}

pub struct SesFactory {
    http: reqwest::Client,
}

impl SesFactory {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                // A signed request goes to a fixed host we constructed; a 3xx
                // is never legitimate and following one would replay the
                // credentials somewhere we didn't sign for.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("reqwest client builds"),
        }
    }
}

impl Default for SesFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl EmailSenderFactory for SesFactory {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn label(&self) -> &'static str {
        "Amazon SES"
    }

    fn secret_keys(&self) -> &'static [&'static str] {
        &["secret_access_key"]
    }

    fn build(&self, config: &Value) -> Result<Arc<dyn EmailSender>, EmailBuildError> {
        let mut cfg: SesConfig = serde_json::from_value(config.clone())
            .map_err(|e| EmailBuildError::Invalid(format!("decode: {e}")))?;

        let required = [
            ("region", &cfg.region),
            ("access_key_id", &cfg.access_key_id),
            ("secret_access_key", &cfg.secret_access_key),
            ("from_address", &cfg.from_address),
        ];
        for (name, value) in required {
            if value.trim().is_empty() {
                return Err(EmailBuildError::Invalid(format!("{name} is required")));
            }
        }
        // The region becomes part of the hostname we connect to, so it is
        // held to the shape AWS regions actually have.
        if !cfg
            .region
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(EmailBuildError::Invalid(
                "region must look like us-east-1".into(),
            ));
        }
        cfg.from_address = super::validate_address(&cfg.from_address)
            .map_err(|e| EmailBuildError::Invalid(format!("from_address: {e}")))?;
        if let Some(name) = &cfg.from_name {
            let name = name.trim();
            if name.chars().count() > MAX_FROM_NAME_LEN
                || name.chars().any(|c| c.is_control() || "\"\\<>".contains(c))
            {
                return Err(EmailBuildError::Invalid(format!(
                    "from_name must be at most {MAX_FROM_NAME_LEN} characters, without quotes, \
                     backslashes, angle brackets or line breaks"
                )));
            }
        }
        if let Some(set) = &cfg.configuration_set
            && !set
                .trim()
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(EmailBuildError::Invalid(
                "configuration_set may contain only letters, digits, '-' and '_'".into(),
            ));
        }

        Ok(Arc::new(SesSender {
            http: self.http.clone(),
            config: cfg,
        }))
    }
}

pub struct SesSender {
    http: reqwest::Client,
    config: SesConfig,
}

impl SesSender {
    fn host(&self) -> String {
        format!("email.{}.amazonaws.com", self.config.region)
    }

    /// The `FromEmailAddress` value: bare, or `"Name" <addr>` with a non-ASCII
    /// name MIME-encoded, since SES requires RFC 5322 ASCII here.
    fn sender_address(&self) -> String {
        let addr = &self.config.from_address;
        match self.config.from_name.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => {
                if name.is_ascii() {
                    format!("\"{name}\" <{addr}>")
                } else {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(name);
                    format!("=?UTF-8?B?{b64}?= <{addr}>")
                }
            }
            _ => addr.clone(),
        }
    }

    /// The SES v2 `SendEmail` request body.
    fn request_body(&self, email: &OutgoingEmail) -> Value {
        let mut body = json!({
            "FromEmailAddress": self.sender_address(),
            "Destination": { "ToAddresses": [email.to] },
            "Content": {
                "Simple": {
                    "Subject": { "Data": clean_subject(&email.subject), "Charset": "UTF-8" },
                    "Body": { "Text": { "Data": email.text, "Charset": "UTF-8" } },
                }
            },
        });
        if let Some(set) = self.config.configuration_set.as_deref().map(str::trim)
            && !set.is_empty()
        {
            body["ConfigurationSetName"] = json!(set);
        }
        body
    }

    /// Headers to send, `Authorization` included, for `payload` at `now`.
    fn signed_headers(&self, payload: &[u8], now: DateTime<Utc>) -> BTreeMap<String, String> {
        let payload_hash = hex_sha256(payload);
        let mut headers = BTreeMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert("host".to_string(), self.host());
        headers.insert("x-amz-content-sha256".to_string(), payload_hash.clone());
        headers.insert(
            "x-amz-date".to_string(),
            now.format("%Y%m%dT%H%M%SZ").to_string(),
        );
        let auth = authorization(
            &self.config.access_key_id,
            &self.config.secret_access_key,
            &self.config.region,
            SERVICE,
            "POST",
            SEND_PATH,
            &headers,
            &payload_hash,
            now,
        );
        headers.insert("authorization".to_string(), auth);
        headers
    }
}

#[async_trait]
impl EmailSender for SesSender {
    fn kind(&self) -> &'static str {
        KIND
    }

    async fn send(&self, email: &OutgoingEmail) -> Result<(), EmailError> {
        let to = super::validate_address(&email.to).map_err(EmailError::Invalid)?;
        let email = OutgoingEmail {
            to,
            ..email.clone()
        };
        let payload = serde_json::to_vec(&self.request_body(&email))
            .map_err(|e| EmailError::Invalid(format!("encode: {e}")))?;
        let headers = self.signed_headers(&payload, Utc::now());

        let url = format!("https://{}{SEND_PATH}", self.host());
        let mut req = self.http.post(&url).body(payload);
        for (name, value) in &headers {
            // reqwest derives `host` from the URL; sending it twice would be
            // rejected by some proxies.
            if name != "host" {
                req = req.header(name, value);
            }
        }
        let res = req
            .send()
            .await
            .map_err(|e| EmailError::Transient(format!("could not reach SES: {e}")))?;
        let status = res.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(());
        }
        let error_type = res
            .headers()
            .get("x-amzn-errortype")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = res.text().await.unwrap_or_default();
        Err(classify_failure(status, error_type.as_deref(), &body))
    }
}

/// Collapse line breaks and control characters, then cap the length. The
/// subject is built from admin-chosen text, but a header is the one place a
/// stray newline matters, so it never reaches SES with one.
fn clean_subject(subject: &str) -> String {
    let flat: String = subject
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > MAX_SUBJECT_LEN {
        let mut cut: String = flat.chars().take(MAX_SUBJECT_LEN - 1).collect();
        cut.push('…');
        cut
    } else {
        flat
    }
}

/// Build the SigV4 `Authorization` header for a header-signed request.
/// `headers` holds every header to sign, names lower-case.
#[allow(clippy::too_many_arguments)]
fn authorization(
    access_key_id: &str,
    secret: &str,
    region: &str,
    service: &str,
    method: &str,
    path: &str,
    headers: &BTreeMap<String, String>,
    payload_hash: &str,
    now: DateTime<Utc>,
) -> String {
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let scope = sigv4::credential_scope(&date_stamp, region, service);
    let canonical = canonical_request(method, path, headers, payload_hash);
    let string_to_sign = format!(
        "{ALGORITHM}\n{amz_date}\n{scope}\n{}",
        hex_sha256(canonical.as_bytes())
    );
    let signature = hex(&sigv4::sign(
        secret,
        &date_stamp,
        region,
        service,
        string_to_sign.as_bytes(),
    ));
    let signed = headers.keys().cloned().collect::<Vec<_>>().join(";");
    format!(
        "{ALGORITHM} Credential={access_key_id}/{scope}, SignedHeaders={signed}, Signature={signature}"
    )
}

/// Canonical request for a query-less request signed in the header. Header
/// values are trimmed; `BTreeMap` order is the required lexical order.
fn canonical_request(
    method: &str,
    path: &str,
    headers: &BTreeMap<String, String>,
    payload_hash: &str,
) -> String {
    let canonical_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{k}:{}\n", v.trim()))
        .collect();
    let signed = headers.keys().cloned().collect::<Vec<_>>().join(";");
    format!(
        "{method}\n{}\n\n{canonical_headers}\n{signed}\n{payload_hash}",
        sigv4::uri_encode(path, true)
    )
}

/// Map a non-2xx SES response to a retry decision and an admin-facing message.
///
/// The error type comes from the `x-amzn-ErrorType` header (sometimes with a
/// `:http://…` suffix) or, failing that, the JSON body's `__type`/`code`.
fn classify_failure(status: u16, error_type: Option<&str>, body: &str) -> EmailError {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let field = |k: &str| {
        parsed
            .as_ref()
            .and_then(|v| v.get(k))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let code = error_type
        .map(str::to_string)
        .or_else(|| field("__type"))
        .or_else(|| field("code"))
        .map(|c| {
            // `MessageRejected:http://internal.amazon.com/…` or
            // `com.amazonaws…#MessageRejected` → `MessageRejected`.
            let c = c.split(':').next().unwrap_or("").to_string();
            c.rsplit('#').next().unwrap_or("").to_string()
        })
        .unwrap_or_default();
    let message = field("message")
        .or_else(|| field("Message"))
        .unwrap_or_default();
    let detail = if message.is_empty() {
        String::new()
    } else {
        format!(" ({message})")
    };

    match code.as_str() {
        "TooManyRequestsException" | "ThrottlingException" | "LimitExceededException" => {
            return EmailError::Transient(format!("SES is throttling sends{detail}"));
        }
        "MessageRejected" => {
            return EmailError::Permanent(format!(
                "SES rejected the message — check that the sender identity is verified and, \
                 if the account is still in the SES sandbox, that the recipient is verified too\
                 {detail}"
            ));
        }
        "MailFromDomainNotVerifiedException" => {
            return EmailError::Permanent(format!(
                "the sender's MAIL FROM domain is not verified in SES{detail}"
            ));
        }
        "AccountSuspendedException" | "SendingPausedException" => {
            return EmailError::Permanent(format!(
                "sending is paused or suspended on this SES account{detail}"
            ));
        }
        "UnrecognizedClientException"
        | "InvalidSignatureException"
        | "SignatureDoesNotMatch"
        | "IncompleteSignature"
        | "InvalidClientTokenId" => {
            return EmailError::Permanent("the access key or secret is wrong".into());
        }
        "AccessDeniedException" => {
            return EmailError::Permanent(
                "the credentials are valid but lack ses:SendEmail for this identity".into(),
            );
        }
        "NotFoundException" => {
            return EmailError::Permanent(format!(
                "SES could not find a referenced resource — check the configuration set{detail}"
            ));
        }
        _ => {}
    }

    if status == 429 || status >= 500 {
        return EmailError::Transient(format!("SES returned HTTP {status}{detail}"));
    }
    let named = if code.is_empty() {
        format!("HTTP {status}")
    } else {
        code
    };
    EmailError::Permanent(format!("SES rejected the message ({named}){detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn config() -> SesConfig {
        SesConfig {
            region: "us-east-1".into(),
            access_key_id: "AKIDEXAMPLE".into(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            from_address: "forms@example.com".into(),
            from_name: None,
            configuration_set: None,
        }
    }

    fn sender(config: SesConfig) -> SesSender {
        SesSender {
            http: reqwest::Client::new(),
            config,
        }
    }

    /// AWS's SigV4 test suite, `post-x-www-form-urlencoded`: the canonical
    /// request and final signature for a header-signed POST. Pins header
    /// canonicalisation, the empty query line and the payload hash — the half
    /// of the algorithm `sigv4.rs` doesn't own.
    #[test]
    fn authorization_matches_published_aws_test_vector() {
        let when = Utc.with_ymd_and_hms(2015, 8, 30, 12, 36, 0).unwrap();
        let mut headers = BTreeMap::new();
        headers.insert(
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        );
        headers.insert("host".to_string(), "example.amazonaws.com".to_string());
        headers.insert("x-amz-date".to_string(), "20150830T123600Z".to_string());
        let payload_hash = hex_sha256(b"Param1=value1");

        let cr = canonical_request("POST", "/", &headers, &payload_hash);
        assert_eq!(
            cr,
            "POST\n/\n\n\
content-type:application/x-www-form-urlencoded\n\
host:example.amazonaws.com\n\
x-amz-date:20150830T123600Z\n\n\
content-type;host;x-amz-date\n\
9095672bbd1f56dfc5b65f3e153adc8731a4a654192329106275f4c7b24d0b6e"
        );

        let auth = authorization(
            "AKIDEXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "us-east-1",
            "service",
            "POST",
            "/",
            &headers,
            &payload_hash,
            when,
        );
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
SignedHeaders=content-type;host;x-amz-date, \
Signature=ff11897932ad3f4e8b18135d722051e5ac45fc38421b1da7b9d196a0fe09473a"
        );
    }

    #[test]
    fn send_request_signs_content_hash_and_ses_scope() {
        let s = sender(config());
        let when = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        let headers = s.signed_headers(b"{}", when);
        assert_eq!(headers["host"], "email.us-east-1.amazonaws.com");
        assert_eq!(headers["x-amz-date"], "20260102T030405Z");
        assert_eq!(headers["x-amz-content-sha256"], hex_sha256(b"{}"));
        let auth = &headers["authorization"];
        assert!(
            auth.starts_with(
                "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20260102/us-east-1/ses/aws4_request, \
SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date, Signature="
            ),
            "{auth}"
        );
    }

    #[test]
    fn request_body_is_a_plain_text_simple_message() {
        let mut cfg = config();
        cfg.configuration_set = Some("notifications".into());
        let body = sender(cfg).request_body(&OutgoingEmail {
            to: "ops@example.com".into(),
            subject: "New submission: Contact".into(),
            text: "Email: a@b.co\n".into(),
        });
        assert_eq!(
            body,
            json!({
                "FromEmailAddress": "forms@example.com",
                "Destination": { "ToAddresses": ["ops@example.com"] },
                "Content": { "Simple": {
                    "Subject": { "Data": "New submission: Contact", "Charset": "UTF-8" },
                    "Body": { "Text": { "Data": "Email: a@b.co\n", "Charset": "UTF-8" } },
                }},
                "ConfigurationSetName": "notifications",
            })
        );
        // No HTML part, ever: visitor values must never be parsed as markup.
        assert!(body["Content"]["Simple"]["Body"].get("Html").is_none());
    }

    #[test]
    fn from_name_is_quoted_or_mime_encoded() {
        let mut cfg = config();
        cfg.from_name = Some("Open Relay".into());
        assert_eq!(
            sender(cfg.clone()).sender_address(),
            "\"Open Relay\" <forms@example.com>"
        );
        cfg.from_name = Some("Café".into());
        assert_eq!(
            sender(cfg).sender_address(),
            "=?UTF-8?B?Q2Fmw6k=?= <forms@example.com>"
        );
    }

    #[test]
    fn subject_never_carries_a_line_break() {
        assert_eq!(
            clean_subject("New\r\nBcc: x@evil.com"),
            "New Bcc: x@evil.com"
        );
        assert_eq!(
            clean_subject(&"x".repeat(500)).chars().count(),
            MAX_SUBJECT_LEN
        );
    }

    #[test]
    fn classifies_throttling_and_server_errors_as_transient() {
        assert!(matches!(
            classify_failure(429, Some("TooManyRequestsException"), "{}"),
            EmailError::Transient(_)
        ));
        assert!(matches!(
            classify_failure(400, None, r#"{"__type":"LimitExceededException"}"#),
            EmailError::Transient(_)
        ));
        assert!(matches!(
            classify_failure(503, None, ""),
            EmailError::Transient(_)
        ));
    }

    #[test]
    fn classifies_rejections_as_permanent_with_an_actionable_message() {
        let e = classify_failure(
            400,
            Some("MessageRejected:http://internal.amazon.com/coral/com.amazonaws.sesv2/"),
            r#"{"message":"Email address is not verified."}"#,
        );
        let EmailError::Permanent(msg) = e else {
            panic!("expected permanent, got {e:?}")
        };
        assert!(msg.contains("sandbox"), "{msg}");
        assert!(msg.contains("Email address is not verified."), "{msg}");

        assert_eq!(
            classify_failure(403, Some("UnrecognizedClientException"), "{}"),
            EmailError::Permanent("the access key or secret is wrong".into())
        );
        assert!(matches!(
            classify_failure(400, Some("BadRequestException"), "{}"),
            EmailError::Permanent(_)
        ));
        assert!(matches!(
            classify_failure(404, None, ""),
            EmailError::Permanent(_)
        ));
    }

    #[test]
    fn factory_rejects_incomplete_or_unsafe_config() {
        let f = SesFactory::new();
        assert!(f.build(&json!({})).is_err());
        let ok = json!({
            "region": "us-east-1", "access_key_id": "a",
            "secret_access_key": "s", "from_address": "forms@example.com",
        });
        assert!(f.build(&ok).is_ok());

        let mut bad = ok.clone();
        bad["secret_access_key"] = json!("");
        assert!(f.build(&bad).is_err());

        let mut bad = ok.clone();
        bad["region"] = json!("evil.com/x");
        assert!(f.build(&bad).is_err());

        let mut bad = ok.clone();
        bad["from_address"] = json!("a@example.com, b@example.com");
        assert!(f.build(&bad).is_err());

        let mut bad = ok.clone();
        bad["from_name"] = json!("x\r\nBcc: y@evil.com");
        assert!(f.build(&bad).is_err());
    }

    #[test]
    fn factory_declares_exactly_one_secret_key() {
        assert_eq!(SesFactory::new().secret_keys(), &["secret_access_key"]);
    }
}
