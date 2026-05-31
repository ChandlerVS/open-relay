//! GoHighLevel contact-upsert backend.
//!
//! Auth: Private Integration Token (PIT) — a long-lived bearer the admin
//! pastes from a location's "Private Integrations" page. No OAuth dance.
//!
//! Endpoint: `POST https://services.leadconnectorhq.com/contacts/upsert`
//! (LeadConnector v2 API; `Version: 2021-07-28` header). The upsert is
//! idempotent on the GHL side — duplicate detection runs against the
//! location's configured priority sequence (email/phone), so we don't
//! perform any client-side dedupe.
//!
//! Field mapping (OpenRelay standard key → GHL body key):
//!
//! | OpenRelay        | GoHighLevel       |
//! |------------------|-------------------|
//! | first_name       | firstName         |
//! | last_name        | lastName          |
//! | email            | email             |
//! | phone            | phone             |
//! | company          | companyName       |
//! | website          | website           |
//! | address_line_1   | address1          |
//! | city             | city              |
//! | state            | state             |
//! | postal_code      | postalCode        |
//! | country          | country           |
//! | job_title        | customFields[]    |
//! | message          | customFields[]    |
//! | address_line_2   | customFields[]    |
//! | <custom keys>    | customFields[]    |
//!
//! All custom fields are sent as `{ "key": "<openrelay_key>", "field_value": <value> }`
//! entries — GHL accepts both numeric IDs and string keys here, and string
//! keys keep configuration in OpenRelay rather than mirroring GHL's id
//! catalog (admins can map them on the GHL side using their custom-field
//! "Unique Key").

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tracing::warn;

use super::{Backend, BackendBuildError, BackendFactory, DeliveryError, DeliveryPayload};

pub const KIND: &str = "gohighlevel";
const BASE_URL: &str = "https://services.leadconnectorhq.com";
const API_VERSION: &str = "2021-07-28";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Wire shape of the `backend_instance.config` JSON for a GoHighLevel row.
///
/// `private_integration_token` is stored plaintext in v1. TODO: AEAD-encrypt
/// with an env-derived key alongside `oauth_provider_config.client_secret`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GoHighLevelConfig {
    pub location_id: String,
    pub private_integration_token: String,
}

/// Maps the OpenRelay standard key to the GHL camelCase counterpart for
/// top-level body fields. Returns `None` for keys that should be routed
/// into `customFields` instead.
fn top_level_key(open_relay_key: &str) -> Option<&'static str> {
    Some(match open_relay_key {
        "first_name" => "firstName",
        "last_name" => "lastName",
        "email" => "email",
        "phone" => "phone",
        "company" => "companyName",
        "website" => "website",
        "address_line_1" => "address1",
        "city" => "city",
        "state" => "state",
        "postal_code" => "postalCode",
        "country" => "country",
        _ => return None,
    })
}

pub struct GoHighLevelFactory {
    http: reqwest::Client,
}

impl GoHighLevelFactory {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("reqwest client builds with default config");
        Self { http }
    }
}

impl Default for GoHighLevelFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl BackendFactory for GoHighLevelFactory {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn secret_keys(&self) -> &'static [&'static str] {
        &["private_integration_token"]
    }

    fn build(&self, config: &Value) -> Result<Arc<dyn Backend>, BackendBuildError> {
        let cfg: GoHighLevelConfig = serde_json::from_value(config.clone())
            .map_err(|e| BackendBuildError::Invalid(format!("decode: {e}")))?;
        if cfg.location_id.trim().is_empty() {
            return Err(BackendBuildError::Invalid("location_id is empty".into()));
        }
        if cfg.private_integration_token.trim().is_empty() {
            return Err(BackendBuildError::Invalid(
                "private_integration_token is empty".into(),
            ));
        }
        Ok(Arc::new(GoHighLevelBackend {
            http: self.http.clone(),
            config: cfg,
        }))
    }
}

pub struct GoHighLevelBackend {
    http: reqwest::Client,
    config: GoHighLevelConfig,
}

impl GoHighLevelBackend {
    fn build_body(&self, payload: &DeliveryPayload) -> Value {
        let mut body = Map::new();
        body.insert(
            "locationId".to_string(),
            Value::String(self.config.location_id.clone()),
        );
        body.insert("source".to_string(), Value::String("OpenRelay".to_string()));

        let mut custom_fields: Vec<Value> = Vec::new();
        if let Value::Object(data) = &payload.data {
            for (key, value) in data {
                if value.is_null() {
                    continue;
                }
                if let Some(top) = top_level_key(key) {
                    body.insert(top.to_string(), value.clone());
                } else {
                    custom_fields.push(json!({
                        "key": key,
                        "field_value": value,
                    }));
                }
            }
        }
        if !custom_fields.is_empty() {
            body.insert("customFields".to_string(), Value::Array(custom_fields));
        }
        Value::Object(body)
    }
}

#[async_trait]
impl Backend for GoHighLevelBackend {
    fn name(&self) -> &'static str {
        KIND
    }

    async fn deliver(&self, payload: &DeliveryPayload) -> Result<(), DeliveryError> {
        let body = self.build_body(payload);
        let url = format!("{BASE_URL}/contacts/upsert");
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.config.private_integration_token)
            .header("Version", API_VERSION)
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                DeliveryError::Transient(format!("network error contacting gohighlevel: {e}"))
            })?;

        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let body_text = resp
            .text()
            .await
            .unwrap_or_else(|_| "<no response body>".to_string());
        let code = status.as_u16();
        let snippet = body_text.chars().take(500).collect::<String>();
        match code {
            401 | 403 => Err(DeliveryError::Permanent(format!(
                "gohighlevel authentication failed ({code}): {snippet}"
            ))),
            404 => Err(DeliveryError::Permanent(format!(
                "gohighlevel location not found ({code}): {snippet}"
            ))),
            400 | 422 => Err(DeliveryError::Permanent(format!(
                "gohighlevel rejected payload ({code}): {snippet}"
            ))),
            408 | 429 => Err(DeliveryError::Transient(format!(
                "gohighlevel transient ({code}): {snippet}"
            ))),
            500..=599 => Err(DeliveryError::Transient(format!(
                "gohighlevel server error ({code}): {snippet}"
            ))),
            _ => {
                warn!(
                    code,
                    body = %snippet,
                    "unexpected gohighlevel status; treating as permanent"
                );
                Err(DeliveryError::Permanent(format!(
                    "gohighlevel unexpected status ({code}): {snippet}"
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload(data: Value) -> DeliveryPayload {
        DeliveryPayload {
            submission_id: 1,
            form_id: 1,
            data,
        }
    }

    fn backend() -> GoHighLevelBackend {
        GoHighLevelBackend {
            http: reqwest::Client::new(),
            config: GoHighLevelConfig {
                location_id: "loc_abc".to_string(),
                private_integration_token: "pit_xyz".to_string(),
            },
        }
    }

    #[test]
    fn body_maps_standard_keys_to_camel_case() {
        let body = backend().build_body(&payload(json!({
            "first_name": "Ada",
            "last_name": "Lovelace",
            "email": "ada@example.com",
            "phone": "+15551112222",
            "company": "Analytical Engines",
            "website": "https://ada.example",
            "address_line_1": "1 King's Road",
            "city": "London",
            "state": "England",
            "postal_code": "SW1",
            "country": "UK",
        })));
        let obj = body.as_object().unwrap();
        assert_eq!(obj["locationId"], "loc_abc");
        assert_eq!(obj["source"], "OpenRelay");
        assert_eq!(obj["firstName"], "Ada");
        assert_eq!(obj["lastName"], "Lovelace");
        assert_eq!(obj["email"], "ada@example.com");
        assert_eq!(obj["phone"], "+15551112222");
        assert_eq!(obj["companyName"], "Analytical Engines");
        assert_eq!(obj["website"], "https://ada.example");
        assert_eq!(obj["address1"], "1 King's Road");
        assert_eq!(obj["city"], "London");
        assert_eq!(obj["state"], "England");
        assert_eq!(obj["postalCode"], "SW1");
        assert_eq!(obj["country"], "UK");
        assert!(obj.get("customFields").is_none());
    }

    #[test]
    fn body_pushes_non_standard_keys_into_custom_fields() {
        let body = backend().build_body(&payload(json!({
            "first_name": "Ada",
            "message": "hi there",
            "job_title": "Mathematician",
            "address_line_2": "Unit 4",
            "favorite_color": "violet",
        })));
        let obj = body.as_object().unwrap();
        assert_eq!(obj["firstName"], "Ada");
        let custom = obj["customFields"].as_array().unwrap();
        let keys: Vec<&str> = custom
            .iter()
            .map(|v| v["key"].as_str().unwrap())
            .collect();
        assert!(keys.contains(&"message"));
        assert!(keys.contains(&"job_title"));
        assert!(keys.contains(&"address_line_2"));
        assert!(keys.contains(&"favorite_color"));
    }

    #[test]
    fn body_skips_null_values() {
        let body = backend().build_body(&payload(json!({
            "first_name": "Ada",
            "phone": serde_json::Value::Null,
        })));
        let obj = body.as_object().unwrap();
        assert_eq!(obj["firstName"], "Ada");
        assert!(obj.get("phone").is_none());
    }

    #[test]
    fn factory_rejects_empty_token() {
        let factory = GoHighLevelFactory::new();
        match factory.build(&json!({ "location_id": "loc", "private_integration_token": "" })) {
            Err(BackendBuildError::Invalid(_)) => {}
            Ok(_) => panic!("expected invalid config"),
        }
    }

    #[test]
    fn factory_rejects_empty_location() {
        let factory = GoHighLevelFactory::new();
        match factory.build(&json!({ "location_id": "", "private_integration_token": "pit" })) {
            Err(BackendBuildError::Invalid(_)) => {}
            Ok(_) => panic!("expected invalid config"),
        }
    }
}
