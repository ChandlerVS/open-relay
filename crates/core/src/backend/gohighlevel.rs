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
//! Custom fields are resolved to GoHighLevel field **ids** before sending.
//! The admin configures an OpenRelay custom field key that names the GHL
//! field — its "Unique Key" (`contact.billing_city`, with or without the
//! `contact.` prefix and with or without the `{{…}}` braces the UI prints),
//! or its display name. At delivery time the backend reads the location's
//! catalog (`GET /locations/{id}/customFields`, cached for
//! [`FIELD_CACHE_TTL`]) and emits
//! `{ "id": "<ghl id>", "key": "<bare key>", "field_value": <value> }`.
//!
//! Sending the id is what makes this work at all. `id` is the only required
//! member of the entry schema, and GHL matches a `key` against the field's
//! *bare* key — never the `contact.`-prefixed form the UI displays — while
//! answering an entry it cannot resolve with a plain `200` and no record of
//! the value. That silence is why the resolution failure is logged loudly
//! and reported per key rather than shrugged off.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tracing::warn;

use super::{Backend, BackendBuildError, BackendFactory, DeliveryError, DeliveryPayload};

pub const KIND: &str = "gohighlevel";
const BASE_URL: &str = "https://services.leadconnectorhq.com";
const API_VERSION: &str = "2021-07-28";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a location's custom-field catalog is reused before being re-read.
/// Long enough that a burst of deliveries costs one extra request, short
/// enough that a field added in the GHL UI starts landing without a restart.
pub const FIELD_CACHE_TTL: Duration = Duration::from_secs(300);

/// Wire shape of the `backend_instance.config` JSON for a GoHighLevel row.
///
/// `private_integration_token` is AEAD-encrypted at rest (see
/// [`crate::crypto::SecretCipher`]); it is decrypted into this plaintext shape
/// only just before `build`, so the value held here is always plaintext.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GoHighLevelConfig {
    pub location_id: String,
    pub private_integration_token: String,
}

/// Normalizes a user-entered country into the ISO 3166-1 alpha-2 code that
/// GoHighLevel's contact API expects (e.g. `US`). Submitters routinely type a
/// full name (`United States`) or the alpha-3 code (`USA`), both of which GHL
/// rejects, failing the whole delivery.
///
/// The lookup is case-insensitive and ignores surrounding whitespace. An
/// already-valid two-letter code passes straight through (upper-cased), and any
/// value we don't recognize is returned untouched so we never silently drop a
/// legitimate-but-unmapped country — better to forward it and let GHL decide.
fn normalize_country(raw: &str) -> String {
    let trimmed = raw.trim();
    let key = trimmed.to_ascii_uppercase();
    let mapped = match key.as_str() {
        "US" | "USA" | "U.S." | "U.S.A." | "UNITED STATES"
        | "UNITED STATES OF AMERICA" | "AMERICA" => "US",
        "CA" | "CAN" | "CANADA" => "CA",
        "GB" | "UK" | "GBR" | "UNITED KINGDOM" | "GREAT BRITAIN" | "ENGLAND"
        | "SCOTLAND" | "WALES" | "NORTHERN IRELAND" => "GB",
        "AU" | "AUS" | "AUSTRALIA" => "AU",
        "NZ" | "NZL" | "NEW ZEALAND" => "NZ",
        "IE" | "IRL" | "IRELAND" => "IE",
        "DE" | "DEU" | "GER" | "GERMANY" => "DE",
        "FR" | "FRA" | "FRANCE" => "FR",
        "ES" | "ESP" | "SPAIN" => "ES",
        "IT" | "ITA" | "ITALY" => "IT",
        "NL" | "NLD" | "NETHERLANDS" => "NL",
        "MX" | "MEX" | "MEXICO" => "MX",
        "IN" | "IND" | "INDIA" => "IN",
        _ => {
            // Already a two-letter code we don't have a name for: pass the
            // upper-cased form. Otherwise forward the original value verbatim.
            if key.len() == 2 && key.chars().all(|c| c.is_ascii_alphabetic()) {
                return key;
            }
            return trimmed.to_string();
        }
    };
    mapped.to_string()
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

/// Reduces a GHL custom-field identifier to the form both sides of the
/// lookup can agree on: lower-cased, stripped of the `{{…}}` braces the
/// "Unique Key" column prints, and stripped of the `contact.` / `opportunity.`
/// model prefix that the catalog carries but an entry's `key` must not.
///
/// So `{{contact.billing_city}}`, `contact.billing_city` and `billing_city`
/// all resolve to the same field, which is the point — the admin pastes
/// whichever form the GHL UI put in front of them.
fn normalize_field_key(raw: &str) -> String {
    let mut k = raw.trim();
    k = k.trim_start_matches("{{").trim_end_matches("}}").trim();
    let lowered = k.to_ascii_lowercase();
    for prefix in ["contact.", "opportunity."] {
        if let Some(rest) = lowered.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    lowered
}

/// One location's custom-field catalog, flattened to `normalized key -> id`.
type FieldIndex = Arc<HashMap<String, String>>;

struct CacheEntry {
    fetched_at: Instant,
    index: FieldIndex,
}

/// Per-location catalog cache, shared by every backend the factory builds.
/// `BackendFactory::build` runs once per delivery, so the cache has to live
/// on the factory (registered once at boot) rather than on the backend.
#[derive(Clone, Default)]
struct FieldCache {
    inner: Arc<Mutex<HashMap<String, CacheEntry>>>,
}

impl FieldCache {
    fn get(&self, location_id: &str) -> Option<FieldIndex> {
        let guard = self.inner.lock().ok()?;
        let entry = guard.get(location_id)?;
        (entry.fetched_at.elapsed() < FIELD_CACHE_TTL).then(|| Arc::clone(&entry.index))
    }

    fn put(&self, location_id: &str, index: FieldIndex) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.insert(
                location_id.to_string(),
                CacheEntry {
                    fetched_at: Instant::now(),
                    index,
                },
            );
        }
    }
}

/// Shape of one entry in `GET /locations/{id}/customFields`. Only the three
/// members the lookup needs are decoded; the rest of the catalog row
/// (dataType, picklistOptions, …) is deliberately ignored.
#[derive(Debug, Deserialize)]
struct CatalogField {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "fieldKey")]
    field_key: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CatalogResponse {
    #[serde(default, rename = "customFields")]
    custom_fields: Vec<CatalogField>,
}

/// Builds the lookup from a catalog response. `fieldKey` wins over `name`:
/// two fields can share a display name, but the key is unique, so a name
/// match is only ever a convenience fallback and never overwrites a real one.
fn index_catalog(fields: Vec<CatalogField>) -> HashMap<String, String> {
    let mut by_key: HashMap<String, String> = HashMap::with_capacity(fields.len());
    let mut by_name: HashMap<String, String> = HashMap::new();
    for f in fields {
        // The catalog carries opportunity fields too; a contact upsert can
        // only address the contact ones.
        if f.model.as_deref().is_some_and(|m| m != "contact") {
            continue;
        }
        if let Some(key) = f.field_key.as_deref() {
            by_key.insert(normalize_field_key(key), f.id.clone());
        }
        if let Some(name) = f.name.as_deref() {
            by_name.entry(normalize_field_key(name)).or_insert(f.id);
        }
    }
    for (name, id) in by_name {
        by_key.entry(name).or_insert(id);
    }
    by_key
}

pub struct GoHighLevelFactory {
    http: reqwest::Client,
    fields: FieldCache,
}

impl GoHighLevelFactory {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            // No redirect following: the target host is a fixed constant, so a
            // 3xx pointing elsewhere is never legitimate.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("reqwest client builds with default config");
        Self {
            http,
            fields: FieldCache::default(),
        }
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
            fields: self.fields.clone(),
        }))
    }
}

pub struct GoHighLevelBackend {
    http: reqwest::Client,
    config: GoHighLevelConfig,
    fields: FieldCache,
}

impl GoHighLevelBackend {
    /// Reads the location's contact custom-field catalog, memoised per
    /// location for [`FIELD_CACHE_TTL`].
    ///
    /// Returns an empty index rather than an error when the catalog can't be
    /// read — a PIT without the `locations.readonly` scope is the common case,
    /// and losing the whole contact over an unresolvable *custom* field would
    /// be a worse outcome than delivering it with those fields keyed only.
    /// Every such miss is warned about, because GHL itself will not complain.
    async fn field_index(&self) -> FieldIndex {
        let location = &self.config.location_id;
        if let Some(hit) = self.fields.get(location) {
            return hit;
        }
        let url = format!("{BASE_URL}/locations/{location}/customFields");
        let fetched = async {
            let resp = self
                .http
                .get(&url)
                .bearer_auth(&self.config.private_integration_token)
                .header("Version", API_VERSION)
                .header("Accept", "application/json")
                .query(&[("model", "contact")])
                .send()
                .await
                .map_err(|e| format!("network error: {e}"))?;
            let status = resp.status();
            let text = resp.text().await.map_err(|e| format!("read body: {e}"))?;
            if !status.is_success() {
                let snippet = text.chars().take(300).collect::<String>();
                return Err(format!("status {}: {snippet}", status.as_u16()));
            }
            serde_json::from_str::<CatalogResponse>(&text).map_err(|e| format!("decode: {e}"))
        }
        .await;

        match fetched {
            Ok(catalog) => {
                let index: FieldIndex = Arc::new(index_catalog(catalog.custom_fields));
                self.fields.put(location, Arc::clone(&index));
                index
            }
            Err(err) => {
                warn!(
                    location_id = %location,
                    error = %err,
                    "could not read gohighlevel custom-field catalog; custom fields \
                     will be sent without a field id and may be silently discarded \
                     (the PIT needs the locations.readonly scope)"
                );
                // Deliberately not cached: a scope fix or a transient blip
                // should take effect on the next delivery, not in five minutes.
                Arc::new(HashMap::new())
            }
        }
    }

    fn build_body(&self, payload: &DeliveryPayload, fields: &FieldIndex) -> Value {
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
                    // GHL wants ISO alpha-2 country codes; normalize free-text
                    // values like "USA" or "United States" before sending.
                    if key == "country" {
                        if let Value::String(raw) = value {
                            body.insert(top.to_string(), Value::String(normalize_country(raw)));
                            continue;
                        }
                    }
                    body.insert(top.to_string(), value.clone());
                } else {
                    // The entry's `key` is the bare form too, never the
                    // prefixed/braced one — it rides along with the id so a
                    // request stays self-describing in a support ticket.
                    let bare = normalize_field_key(key);
                    match fields.get(&bare) {
                        Some(id) => custom_fields.push(json!({
                            "id": id,
                            "key": bare,
                            "field_value": value,
                        })),
                        None => {
                            // GHL answers an unresolvable entry with a 200 and
                            // no stored value, so this warning is the only
                            // signal the admin will ever get.
                            warn!(
                                submission_id = payload.submission_id,
                                form_id = payload.form_id,
                                location_id = %self.config.location_id,
                                field_key = %key,
                                "no gohighlevel custom field matches this key; \
                                 sending it by key alone"
                            );
                            custom_fields.push(json!({
                                "key": bare,
                                "field_value": value,
                            }));
                        }
                    }
                }
            }
        }
        if !custom_fields.is_empty() {
            body.insert("customFields".to_string(), Value::Array(custom_fields));
        }
        if !payload.tags.is_empty() {
            let tags: Vec<Value> = payload
                .tags
                .iter()
                .map(|t| Value::String(t.clone()))
                .collect();
            body.insert("tags".to_string(), Value::Array(tags));
        }
        // Assign the contact owner when the submission was attributed to a rep
        // carrying a GHL user id. `assignedTo` takes a LeadConnector user id.
        if let Some(owner) = payload.assigned_to.as_ref().filter(|s| !s.trim().is_empty()) {
            body.insert("assignedTo".to_string(), Value::String(owner.clone()));
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
        let fields = self.field_index().await;
        let body = self.build_body(payload, &fields);
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
            tags: Vec::new(),
            assigned_to: None,
        }
    }

    fn backend() -> GoHighLevelBackend {
        GoHighLevelBackend {
            http: reqwest::Client::new(),
            config: GoHighLevelConfig {
                location_id: "loc_abc".to_string(),
                private_integration_token: "pit_xyz".to_string(),
            },
            fields: FieldCache::default(),
        }
    }

    /// No catalog read in a unit test — stand in for one.
    fn index(pairs: &[(&str, &str)]) -> FieldIndex {
        Arc::new(
            pairs
                .iter()
                .map(|(k, v)| (normalize_field_key(k), v.to_string()))
                .collect(),
        )
    }

    fn empty_index() -> FieldIndex {
        Arc::new(HashMap::new())
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
            "country": "United Kingdom",
        })), &empty_index());
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
        assert_eq!(obj["country"], "GB");
        assert!(obj.get("customFields").is_none());
    }

    #[test]
    fn country_is_normalized_to_iso_alpha2() {
        // The reported bug: submitters type "USA", which GHL rejects.
        for raw in ["USA", "usa", "United States", "  United States of America  ", "US"] {
            let body = backend().build_body(&payload(json!({ "country": raw })), &empty_index());
            assert_eq!(
                body.as_object().unwrap()["country"],
                "US",
                "country {raw:?} should normalize to US"
            );
        }
    }

    #[test]
    fn country_unknown_value_passes_through() {
        let body = backend().build_body(&payload(json!({ "country": "Atlantis" })), &empty_index());
        assert_eq!(body.as_object().unwrap()["country"], "Atlantis");
    }

    #[test]
    fn body_pushes_non_standard_keys_into_custom_fields() {
        let body = backend().build_body(&payload(json!({
            "first_name": "Ada",
            "message": "hi there",
            "job_title": "Mathematician",
            "address_line_2": "Unit 4",
            "favorite_color": "violet",
        })), &empty_index());
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
        })), &empty_index());
        let obj = body.as_object().unwrap();
        assert_eq!(obj["firstName"], "Ada");
        assert!(obj.get("phone").is_none());
    }

    #[test]
    fn body_includes_tags_when_present() {
        let mut p = payload(json!({ "first_name": "Ada" }));
        p.tags = vec!["hot-lead".to_string(), "webinar".to_string()];
        let body = backend().build_body(&p, &empty_index());
        let obj = body.as_object().unwrap();
        let tags = obj["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0], "hot-lead");
        assert_eq!(tags[1], "webinar");
    }

    #[test]
    fn body_omits_tags_when_empty() {
        let body = backend().build_body(&payload(json!({ "first_name": "Ada" })), &empty_index());
        let obj = body.as_object().unwrap();
        assert!(obj.get("tags").is_none());
    }

    #[test]
    fn body_includes_assigned_to_when_present() {
        let mut p = payload(json!({ "first_name": "Ada" }));
        p.assigned_to = Some("usr_jane123".to_string());
        let body = backend().build_body(&p, &empty_index());
        let obj = body.as_object().unwrap();
        assert_eq!(obj["assignedTo"], "usr_jane123");
    }

    #[test]
    fn body_omits_assigned_to_when_absent_or_blank() {
        let obj = backend()
            .build_body(&payload(json!({ "first_name": "Ada" })), &empty_index());
        assert!(obj.as_object().unwrap().get("assignedTo").is_none());

        let mut p = payload(json!({ "first_name": "Ada" }));
        p.assigned_to = Some("   ".to_string());
        let body = backend().build_body(&p, &empty_index());
        assert!(body.as_object().unwrap().get("assignedTo").is_none());
    }

    #[test]
    fn custom_fields_carry_the_resolved_ghl_id_and_a_bare_key() {
        // The reported bug: the admin configures the key exactly as GHL's
        // "Unique Key" column prints it, and the whole entry is discarded.
        let idx = index(&[("contact.billing_city", "fld_city")]);
        let body = backend().build_body(
            &payload(json!({ "contact.billing_city": "London" })),
            &idx,
        );
        let custom = body.as_object().unwrap()["customFields"]
            .as_array()
            .unwrap();
        assert_eq!(custom.len(), 1);
        assert_eq!(custom[0]["id"], "fld_city");
        assert_eq!(custom[0]["key"], "billing_city");
        assert_eq!(custom[0]["field_value"], "London");
    }

    #[test]
    fn every_spelling_of_a_key_resolves_to_the_same_field() {
        let idx = index(&[("contact.billing_city", "fld_city")]);
        for spelling in [
            "contact.billing_city",
            "billing_city",
            "{{contact.billing_city}}",
            "Contact.Billing_City",
        ] {
            let body = backend().build_body(&payload(json!({ spelling: "London" })), &idx);
            let custom = body.as_object().unwrap()["customFields"]
                .as_array()
                .unwrap();
            assert_eq!(custom[0]["id"], "fld_city", "spelling {spelling:?}");
        }
    }

    #[test]
    fn unresolved_key_still_ships_by_bare_key() {
        // No id to send, but dropping the answer outright would lose data the
        // admin can still recover from the submission record.
        let body = backend().build_body(
            &payload(json!({ "contact.nope": "value" })),
            &empty_index(),
        );
        let custom = body.as_object().unwrap()["customFields"]
            .as_array()
            .unwrap();
        assert!(custom[0].get("id").is_none());
        assert_eq!(custom[0]["key"], "nope");
        assert_eq!(custom[0]["field_value"], "value");
    }

    #[test]
    fn a_checkbox_group_answer_ships_as_an_array() {
        // GHL's checkbox and multi-option fields take a list for field_value,
        // so the stored array must pass through rather than be flattened.
        let idx = index(&[("contact.equipment", "fld_gear")]);
        let body = backend().build_body(
            &payload(json!({ "equipment": ["Mobile Computers", "Label Printers"] })),
            &idx,
        );
        let custom = body.as_object().unwrap()["customFields"]
            .as_array()
            .unwrap();
        assert_eq!(custom[0]["id"], "fld_gear");
        assert_eq!(custom[0]["field_value"], json!(["Mobile Computers", "Label Printers"]));
    }

    #[test]
    fn catalog_indexes_field_key_over_name_and_skips_opportunities() {
        let catalog: CatalogResponse = serde_json::from_value(json!({
            "customFields": [
                {
                    "id": "fld_city",
                    "name": "Billing City",
                    "fieldKey": "contact.billing_city",
                    "model": "contact",
                },
                {
                    "id": "opp_city",
                    "name": "Billing City",
                    "fieldKey": "opportunity.billing_city",
                    "model": "opportunity",
                },
                {
                    // Older payloads omit `model`; treat those as contact.
                    "id": "fld_po",
                    "name": "Purchase Order",
                    "fieldKey": "contact.purchase_order",
                },
            ]
        }))
        .unwrap();
        let idx = index_catalog(catalog.custom_fields);
        assert_eq!(idx.get("billing_city").map(String::as_str), Some("fld_city"));
        assert_eq!(idx.get("purchase_order").map(String::as_str), Some("fld_po"));
        // The display name is a fallback, and it must not have shadowed the
        // key match with the opportunity field.
        assert!(!idx.values().any(|v| v == "opp_city"));
    }

    #[test]
    fn catalog_falls_back_to_the_display_name() {
        let catalog: CatalogResponse = serde_json::from_value(json!({
            "customFields": [
                { "id": "fld_po", "name": "Purchase Order", "fieldKey": "contact.po_2" }
            ]
        }))
        .unwrap();
        let idx = index_catalog(catalog.custom_fields);
        assert_eq!(idx.get("po_2").map(String::as_str), Some("fld_po"));
        assert_eq!(
            idx.get("purchase order").map(String::as_str),
            Some("fld_po")
        );
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
