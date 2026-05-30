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
    /// snake_case identifier, unique within the form. Used as the
    /// submission key.
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
