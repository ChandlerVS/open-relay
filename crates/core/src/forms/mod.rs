//! Form domain logic: validation, SeaORM persistence, and the wire-contract
//! types (DTOs) that describe what crosses the API boundary.
//!
//! Framework-agnostic — `serde` and `utoipa` are pure metadata libraries, not
//! tied to any HTTP framework.
//!
//! Field configuration is split into two groups:
//!
//! - **Standard fields** — a fixed set of well-known fields (name, email,
//!   address, …) that downstream backends know how to map. Each can be
//!   toggled on/off, marked required, and given a custom display label.
//!   See [`StandardFieldsConfig`].
//! - **Custom fields** — an ordered list of caller-defined fields with a key,
//!   label, input type, optional placeholder/help text, and (for `select`)
//!   options. See [`CustomField`].

pub mod service;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::metadata::MetadataEntry;

/// Standard field keys recognised by the form. These map to the typed
/// columns on `submission` rows once that resource lands; for now they're
/// just config the renderer consumes.
pub const STANDARD_FIELD_KEYS: &[&str] = &[
    "first_name",
    "last_name",
    "email",
    "phone",
    "company",
    "job_title",
    "website",
    "message",
    "address_line_1",
    "address_line_2",
    "city",
    "state",
    "postal_code",
    "country",
];

/// Per-field toggle for a standard field. `label` overrides the renderer's
/// default copy when `Some` and non-empty.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct StandardFieldConfig {
    pub enabled: bool,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl StandardFieldConfig {
    pub fn default_enabled() -> Self {
        Self {
            enabled: true,
            required: false,
            label: None,
        }
    }
    pub fn default_disabled() -> Self {
        Self {
            enabled: false,
            required: false,
            label: None,
        }
    }
}

/// Configuration for the fixed set of standard fields. Each field's key in
/// the JSON must be one of [`STANDARD_FIELD_KEYS`]; unknown keys are
/// rejected at validation time.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct StandardFieldsConfig {
    pub first_name: StandardFieldConfig,
    pub last_name: StandardFieldConfig,
    pub email: StandardFieldConfig,
    pub phone: StandardFieldConfig,
    pub company: StandardFieldConfig,
    pub job_title: StandardFieldConfig,
    pub website: StandardFieldConfig,
    pub message: StandardFieldConfig,
    pub address_line_1: StandardFieldConfig,
    pub address_line_2: StandardFieldConfig,
    pub city: StandardFieldConfig,
    pub state: StandardFieldConfig,
    pub postal_code: StandardFieldConfig,
    pub country: StandardFieldConfig,
}

impl Default for StandardFieldsConfig {
    /// Sensible starting point: name + email enabled+required, everything
    /// else disabled. Admins enable extras as needed.
    fn default() -> Self {
        let on_required = || StandardFieldConfig {
            enabled: true,
            required: true,
            label: None,
        };
        Self {
            first_name: on_required(),
            last_name: on_required(),
            email: on_required(),
            phone: StandardFieldConfig::default_disabled(),
            company: StandardFieldConfig::default_disabled(),
            job_title: StandardFieldConfig::default_disabled(),
            website: StandardFieldConfig::default_disabled(),
            message: StandardFieldConfig::default_enabled(),
            address_line_1: StandardFieldConfig::default_disabled(),
            address_line_2: StandardFieldConfig::default_disabled(),
            city: StandardFieldConfig::default_disabled(),
            state: StandardFieldConfig::default_disabled(),
            postal_code: StandardFieldConfig::default_disabled(),
            country: StandardFieldConfig::default_disabled(),
        }
    }
}

/// HTML input types we expose for custom fields.
///
/// `Select` carries its options on the variant so the renderer can't see a
/// select-typed field without options. `Checkbox` is a single boolean
/// checkbox (multi-select uses `Select`).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum CustomFieldType {
    Text,
    Email,
    Number,
    Tel,
    Url,
    Textarea,
    Select {
        #[serde(default)]
        options: Vec<String>,
    },
    Checkbox,
}

/// One backend destination on a form. Each entry queues one delivery row per
/// submission.
///
/// `kind` matches a backend kind registered in `BackendRegistry` — either a
/// static singleton like `"open-relay"` (in which case `instance_id` is
/// `None`) or a configurable kind like `"gohighlevel"` whose credentials
/// live in `backend_instance` (in which case `instance_id` is `Some`).
///
/// The legacy serde key `"name"` is accepted as an alias for `kind` so JSON
/// written before configurable instances landed still parses.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct BackendBinding {
    #[serde(alias = "name")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<i32>,
}

impl BackendBinding {
    pub fn open_relay() -> Self {
        Self {
            kind: "open-relay".into(),
            instance_id: None,
        }
    }
}

/// Default `backends` for a newly created form: deliver to OpenRelay's own
/// store so the dashboard sees submissions immediately.
pub fn default_backends() -> Vec<BackendBinding> {
    vec![BackendBinding::open_relay()]
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct CustomField {
    /// Identifier, unique within the form. Used verbatim as the submission key
    /// and as the lookup key a backend maps onto its destination field, so it
    /// accepts any format the destination needs (e.g. a GoHighLevel custom-field
    /// unique key or field id) — only whitespace/control chars are rejected.
    pub key: String,
    pub label: String,
    #[serde(flatten)]
    pub kind: CustomFieldType,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help_text: Option<String>,
    /// Render order, ascending. Service code re-sorts on write so callers
    /// can submit in any order.
    #[serde(default)]
    pub position: i32,
}

/// An extra URL query param captured from the QR landing page and emitted as a
/// per-submission tag. The param's value is what gets tagged; `tag_prefix`, when
/// set, is prepended as `"<prefix>:<value>"` (e.g. param `event` with prefix
/// `event` → tag `event:mjbiz-2026`). The reserved `rep` param is handled
/// separately (it resolves to a [`crate::reps`] entry, not a tag here).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct SourceParam {
    pub param: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_prefix: Option<String>,
}

/// Input shape for creating a form. `slug` defaults to a slugified `name`
/// if `None`/empty. `standard_fields` defaults to [`StandardFieldsConfig::default`]
/// if absent. `custom_fields` defaults to empty.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct NewForm {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standard_fields: Option<StandardFieldsConfig>,
    #[serde(default)]
    pub custom_fields: Vec<CustomField>,
    /// Backends to deliver submissions to. Defaults to `[open-relay]` if
    /// omitted. An empty vec is rejected — a form must have at least one
    /// backend or submissions go nowhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backends: Option<Vec<BackendBinding>>,
    /// Tags dispatched to backends alongside every submission from this form.
    /// Defaults to empty.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Sales reps (by [`crate::reps`] id) this form offers. A submission's
    /// `?rep=<key>` is resolved against this set. Defaults to empty.
    #[serde(default)]
    pub reps: Vec<i32>,
    /// Extra URL params to capture as per-submission tags. Defaults to empty.
    #[serde(default)]
    pub source_params: Vec<SourceParam>,
    /// Per-form metadata toggles (e.g. email deduplication). Each entry is
    /// upserted on create; omit (or send an empty list) to leave metadata
    /// unset. See [`crate::metadata`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Vec<MetadataEntry>>,
}

/// Outbound representation of a form. `owner_id` is exposed to admins; the
/// public-facing endpoint uses [`PublicFormDto`] instead.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FormDto {
    pub id: i32,
    pub owner_id: i32,
    pub name: String,
    pub slug: String,
    pub standard_fields: StandardFieldsConfig,
    pub custom_fields: Vec<CustomField>,
    pub backends: Vec<BackendBinding>,
    pub tags: Vec<String>,
    /// Sales reps (by [`crate::reps`] id) this form offers.
    pub reps: Vec<i32>,
    /// Extra URL params captured as per-submission tags.
    pub source_params: Vec<SourceParam>,
    /// Per-form metadata toggles (e.g. email deduplication). See
    /// [`crate::metadata`].
    pub metadata: Vec<MetadataEntry>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Partial update. `None` means "leave the field alone". Custom-field
/// updates replace the entire list (`Some(vec![])` clears all customs).
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
pub struct UpdateForm {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standard_fields: Option<StandardFieldsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_fields: Option<Vec<CustomField>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backends: Option<Vec<BackendBinding>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// `None` leaves reps untouched. `Some(vec![])` clears all associations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reps: Option<Vec<i32>>,
    /// `None` leaves source params untouched. `Some(vec![])` clears them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_params: Option<Vec<SourceParam>>,
    /// `None` leaves metadata untouched. `Some` upserts each entry (so an
    /// explicit `email_deduplication = false` turns the toggle off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Vec<MetadataEntry>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FormList {
    pub items: Vec<FormDto>,
    pub total: u64,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FormSelectOption {
    pub id: i32,
    pub label: String,
}

/// Public, unauthenticated view of a form — the shape consumed by the embed
/// SDK / form renderer. Strips `owner_id` and audit timestamps.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PublicFormDto {
    pub id: i32,
    pub name: String,
    pub slug: String,
    pub standard_fields: StandardFieldsConfig,
    pub custom_fields: Vec<CustomField>,
    pub backends: Vec<BackendBinding>,
}

/// A ready-to-paste embed snippet for a form, returned to admins so they can
/// install the form on their own site. Everything in `snippet` derives from
/// trusted server config plus the form id — there's no caller-supplied input —
/// so it's safe to render verbatim.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EmbedSnippetDto {
    /// The form's numeric id (rendered as `data-form-id`).
    pub form_id: i32,
    /// URL the embed SDK bundle (`open-relay.js`) is served from — the `src`.
    pub sdk_url: String,
    /// Public API base URL the embedded form fetches its schema from and posts
    /// submissions to (rendered as `data-api-url`).
    pub api_url: String,
    /// The full `<script>` tag to copy-paste into a host page's HTML.
    pub snippet: String,
}

impl EmbedSnippetDto {
    /// Assemble the snippet from the form id and the (already-normalised) SDK
    /// and API base URLs. Pure string assembly — no I/O — so it's unit-testable
    /// without a server or database. `data-theme` is omitted intentionally: the
    /// SDK defaults to a static light theme, which a host opts out of explicitly.
    pub fn build(form_id: i32, sdk_url: &str, api_url: &str) -> Self {
        let snippet = format!(
            "<script src=\"{sdk_url}\" data-form-id=\"{form_id}\" data-api-url=\"{api_url}\"></script>"
        );
        Self {
            form_id,
            sdk_url: sdk_url.to_string(),
            api_url: api_url.to_string(),
            snippet,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_snippet_renders_script_tag() {
        let dto = EmbedSnippetDto::build(
            42,
            "https://cdn.example.com/open-relay.js",
            "https://api.example.com",
        );
        assert_eq!(
            dto.snippet,
            "<script src=\"https://cdn.example.com/open-relay.js\" data-form-id=\"42\" data-api-url=\"https://api.example.com\"></script>"
        );
        assert_eq!(dto.form_id, 42);
        assert_eq!(dto.sdk_url, "https://cdn.example.com/open-relay.js");
        assert_eq!(dto.api_url, "https://api.example.com");
    }
}
