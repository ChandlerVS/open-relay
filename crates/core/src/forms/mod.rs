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
pub mod visibility;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::metadata::MetadataEntry;

/// Declares the fixed standard-field set exactly once.
///
/// The key list, the [`StandardFieldsConfig`] struct, and the key-to-field
/// lookups all expand from this single invocation, so adding a standard field
/// here is one edit rather than four kept-in-sync copies. (The `submission`
/// entity columns and the two TS catalogues still have to be updated
/// separately — see `crates/entity/src/submission.rs` and
/// `packages/form-renderer/src/standardFields.ts`.)
macro_rules! declare_standard_fields {
    ($($key:ident),+ $(,)?) => {
        /// Standard field keys recognised by the form, in canonical render
        /// order. These map to the typed columns on `submission` rows.
        pub const STANDARD_FIELD_KEYS: &[&str] = &[$(stringify!($key)),+];

        /// Configuration for the fixed set of standard fields. Each field's key
        /// in the JSON must be one of [`STANDARD_FIELD_KEYS`]; unknown keys are
        /// rejected at validation time.
        #[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
        pub struct StandardFieldsConfig {
            $(pub $key: StandardFieldConfig,)+
        }

        impl StandardFieldsConfig {
            /// Look a field up by wire key. `None` for a key that is not a
            /// standard field.
            pub fn get(&self, key: &str) -> Option<&StandardFieldConfig> {
                match key {
                    $(stringify!($key) => Some(&self.$key),)+
                    _ => None,
                }
            }

            pub fn get_mut(&mut self, key: &str) -> Option<&mut StandardFieldConfig> {
                match key {
                    $(stringify!($key) => Some(&mut self.$key),)+
                    _ => None,
                }
            }

            /// Iterate `(key, config)` pairs in [`STANDARD_FIELD_KEYS`] order.
            pub fn iter(&self) -> impl Iterator<Item = (&'static str, &StandardFieldConfig)> {
                [$((stringify!($key), &self.$key)),+].into_iter()
            }

            pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut StandardFieldConfig> {
                [$(&mut self.$key),+].into_iter()
            }

            /// Every field off. The starting point for the projection built by
            /// `service::legacy_from_layout`.
            pub fn all_disabled() -> Self {
                Self { $($key: StandardFieldConfig::default_disabled(),)+ }
            }
        }
    };
}

declare_standard_fields!(
    first_name,
    last_name,
    email,
    phone,
    company,
    job_title,
    website,
    message,
    address_line_1,
    address_line_2,
    city,
    state,
    postal_code,
    country,
);

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
/// `Select` and `Radio` carry their options on the variant so the renderer
/// can't see an option-typed field without options. `Checkbox` is a single
/// boolean checkbox (multi-select uses `Select`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
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
    /// A radio group. Same option semantics as `Select` — the difference is
    /// purely presentational, so every place that matches `Select` for options
    /// handling matches this too.
    Radio {
        #[serde(default)]
        options: Vec<String>,
    },
    Checkbox,
}

impl CustomFieldType {
    /// The declared options, for the two variants that carry them. Lets option
    /// rules (validation, trimming, membership checks) be written once instead
    /// of once per variant.
    pub fn options(&self) -> Option<&Vec<String>> {
        match self {
            CustomFieldType::Select { options } | CustomFieldType::Radio { options } => {
                Some(options)
            }
            _ => None,
        }
    }

    pub fn options_mut(&mut self) -> Option<&mut Vec<String>> {
        match self {
            CustomFieldType::Select { options } | CustomFieldType::Radio { options } => {
                Some(options)
            }
            _ => None,
        }
    }

    pub fn is_checkbox(&self) -> bool {
        matches!(self, CustomFieldType::Checkbox)
    }
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

/// Whether every condition must hold, or only one of them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    #[default]
    All,
    Any,
}

impl MatchMode {
    /// Lets `all` stay off the wire, so a rule written today matches what a
    /// caller that never heard of `match` would send.
    pub fn is_default(&self) -> bool {
        matches!(self, MatchMode::All)
    }
}

/// How one condition compares a controlling field's answer.
///
/// There is deliberately no `one_of`: "A or B" is a [`MatchMode::Any`] rule with
/// two [`ConditionOp::Equals`] conditions, which keeps `value` a plain
/// `Option<String>` and so keeps `deny_unknown_fields` available on
/// [`Condition`] (an untagged or flattened operand would forbid it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOp {
    Equals,
    NotEquals,
    Contains,
    IsEmpty,
    IsNotEmpty,
    IsChecked,
    IsNotChecked,
}

impl ConditionOp {
    /// Whether this operator takes an operand. The four unary ops must not
    /// carry a `value`; the three binary ones must.
    pub fn takes_value(&self) -> bool {
        matches!(
            self,
            ConditionOp::Equals | ConditionOp::NotEquals | ConditionOp::Contains
        )
    }
}

/// One comparison against a controlling field's answer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    /// Submission key of a field appearing *strictly earlier* in the layout.
    /// Enforced by `service::validate_layout`; see [`VisibilityRule`].
    pub field: String,
    pub op: ConditionOp,
    /// The operand. Required for `equals`/`not_equals`/`contains`, forbidden
    /// for the rest — see [`ConditionOp::takes_value`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// Show an element only when the visitor's earlier answers match.
///
/// Every condition names a field that appears **strictly earlier in the
/// layout** (`service::validate_layout` rejects anything else). That single
/// rule subsumes unknown-key, self-reference and cycle detection, and it is
/// what lets both evaluators — `crate::forms::visibility` here and
/// `visibility.ts` in the renderer — resolve a whole form in one forward pass.
///
/// Rules live only in the `layout` column. A standard element's rule is dropped
/// by the projection in `service::legacy_from_layout`, exactly like its
/// `placeholder`/`width`; a custom field's rides along in `custom_fields`
/// because a `Custom` element's config *is* the `CustomField` JSON. Either way
/// an embed bundle cached before this existed ignores the key and keeps drawing
/// every element — see the module docs on `crate::forms::visibility` for what
/// the server does about that.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct VisibilityRule {
    #[serde(
        rename = "match",
        default,
        skip_serializing_if = "MatchMode::is_default"
    )]
    pub match_mode: MatchMode,
    /// At least one, at most `service::MAX_CONDITIONS`.
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
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
    ///
    /// Advisory once a form has a `layout`: order then comes from the layout
    /// array and this is renumbered to match on write. Retained because
    /// pre-layout API clients still send it.
    #[serde(default)]
    pub position: i32,
    /// Fraction of the row this field occupies. Layout-only — invisible to
    /// pre-layout embed bundles, which always render full width.
    #[serde(default, skip_serializing_if = "FieldWidth::is_default")]
    pub width: FieldWidth,
    /// Value prefilled by the renderer. Never applied server-side: a
    /// submission that omits the key is still treated as absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    /// Show this element only when earlier answers match. `None` is
    /// unconditional. See [`VisibilityRule`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_when: Option<VisibilityRule>,
}

/// Fraction of a row a field occupies when rendered. Layout-only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldWidth {
    #[default]
    Full,
    Half,
}

impl FieldWidth {
    /// Lets `Full` stay off the wire so layout JSON written today matches what
    /// a caller that never heard of widths would send.
    pub fn is_default(&self) -> bool {
        matches!(self, FieldWidth::Full)
    }
}

/// Overrides how a standard field is rendered, where the catalogue default
/// isn't the only sensible choice.
///
/// Currently only meaningful for `country`: `Select` renders the ISO 3166-1
/// list and submits an alpha-2 code, which
/// `crate::backend::gohighlevel::normalize_country` already passes through
/// unchanged. `None` (what legacy derivation produces) keeps the catalogue
/// default, so forms built before this existed keep their free-text input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StandardInputVariant {
    Text,
    Select,
}

/// A standard field placed in a form's layout.
///
/// Presence in the layout *is* "enabled" — there is no `enabled` flag, because
/// an element that isn't in the list isn't rendered. `label` keeps the existing
/// override semantics from [`StandardFieldConfig`]; the remaining fields have
/// no home in the legacy `standard_fields` column and are dropped by the
/// projection (see `service::legacy_from_layout`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StandardElement {
    /// One of [`STANDARD_FIELD_KEYS`].
    pub key: String,
    #[serde(default)]
    pub required: bool,
    /// Overrides the renderer's default copy when `Some` and non-empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help_text: Option<String>,
    #[serde(default, skip_serializing_if = "FieldWidth::is_default")]
    pub width: FieldWidth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_override: Option<StandardInputVariant>,
    /// Show this element only when earlier answers match. `None` is
    /// unconditional. See [`VisibilityRule`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_when: Option<VisibilityRule>,
}

impl StandardElement {
    /// The element a legacy `(key, StandardFieldConfig)` pair derives to.
    pub fn from_legacy(key: &str, cfg: &StandardFieldConfig) -> Self {
        Self {
            key: key.to_string(),
            required: cfg.required,
            label: cfg.label.clone(),
            placeholder: None,
            help_text: None,
            width: FieldWidth::Full,
            default_value: None,
            input_override: None,
            visible_when: None,
        }
    }
}

/// A static heading between fields.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadingElement {
    pub text: String,
    /// Rendered as `h{level}`. Clamped to 1..=6 on write.
    #[serde(default = "default_heading_level")]
    pub level: u8,
    /// Show this element only when earlier answers match. `None` is
    /// unconditional. See [`VisibilityRule`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_when: Option<VisibilityRule>,
}

fn default_heading_level() -> u8 {
    2
}

/// A block of static explanatory copy between fields.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ParagraphElement {
    pub text: String,
    /// Show this element only when earlier answers match. `None` is
    /// unconditional. See [`VisibilityRule`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_when: Option<VisibilityRule>,
}

/// Splits the form into steps. Everything after this break, up to the next one,
/// is one page; `title` names *that* page, not the one before it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageBreakElement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// One entry in a form's ordered layout.
///
/// Adjacently tagged: `element` names the kind and `config` carries the body
/// (`{"element":"divider"}`, `{"element":"custom","config":{…}}`). Adjacent
/// rather than internal tagging for two reasons: it permits
/// `deny_unknown_fields` throughout — internal tagging and `serde(flatten)`
/// each forbid it, so a typo'd key would otherwise save silently as an empty
/// value — and it keeps a `Custom` element's `config` byte-identical to the
/// `CustomField` JSON already stored in the `custom_fields` column, making the
/// derivation and projection in `service` pure moves.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(
    tag = "element",
    content = "config",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FormElement {
    Standard(StandardElement),
    Custom(CustomField),
    Heading(HeadingElement),
    Paragraph(ParagraphElement),
    Divider,
    PageBreak(PageBreakElement),
}

impl FormElement {
    /// The submission key this element contributes, if any. Decoration
    /// elements contribute nothing.
    pub fn field_key(&self) -> Option<&str> {
        match self {
            FormElement::Standard(s) => Some(&s.key),
            FormElement::Custom(c) => Some(&c.key),
            _ => None,
        }
    }

    /// This element's visibility rule, if it has one.
    ///
    /// `Divider` and `PageBreak` always return `None` and can never carry one:
    /// `Divider` is a serde *unit* variant, and under adjacent tagging giving it
    /// a body would require a `config` key that no stored `{"element":"divider"}`
    /// has. A divider left alone between hidden neighbours is collapsed by the
    /// renderer instead. A conditional page break would mean a conditional page
    /// count, which multi-step forms deliberately keep static.
    pub fn visible_when(&self) -> Option<&VisibilityRule> {
        match self {
            FormElement::Standard(s) => s.visible_when.as_ref(),
            FormElement::Custom(c) => c.visible_when.as_ref(),
            FormElement::Heading(h) => h.visible_when.as_ref(),
            FormElement::Paragraph(p) => p.visible_when.as_ref(),
            FormElement::Divider | FormElement::PageBreak(_) => None,
        }
    }

    /// The slot holding this element's rule, for normalization. `None` for the
    /// variants that cannot carry one.
    pub fn visible_when_slot(&mut self) -> Option<&mut Option<VisibilityRule>> {
        match self {
            FormElement::Standard(s) => Some(&mut s.visible_when),
            FormElement::Custom(c) => Some(&mut c.visible_when),
            FormElement::Heading(h) => Some(&mut h.visible_when),
            FormElement::Paragraph(p) => Some(&mut p.visible_when),
            FormElement::Divider | FormElement::PageBreak(_) => None,
        }
    }
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

/// What the renderer does once a submission is accepted.
///
/// Adjacently tagged like [`FormElement`] — `action` discriminates, `config`
/// carries the body — so `deny_unknown_fields` applies to every arm and a
/// typo'd key in a save is a 400 rather than a silently dropped setting.
///
/// There are two variants but three admin-facing choices: "show a message" and
/// "show a message with a submit-another button" are the same terminal state
/// differing only by [`MessageAction::allow_resubmit`], so the confirmation
/// copy lives in exactly one struct.
///
/// [`Self::default`] is the message action with no overrides, which is what a
/// `NULL` `form.post_submission_action` column decodes to — i.e. the behaviour
/// every form had before this existed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(
    tag = "action",
    content = "config",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PostSubmissionAction {
    /// Replace the form with a confirmation message.
    Message(MessageAction),
    /// Navigate the host page to a URL.
    Redirect(RedirectAction),
}

impl Default for PostSubmissionAction {
    fn default() -> Self {
        Self::Message(MessageAction::default())
    }
}

/// How a multi-step form tells the visitor where they are. A form with no
/// [`FormElement::PageBreak`] is single-page, so renderers ignore this entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStyle {
    /// A track-and-fill bar beneath the Back/Next row, with an optional
    /// percentage. The default.
    #[default]
    Bar,
    /// A plain `Step N of M` line above the fields.
    Steps,
    /// No indicator. A page break's `title` still renders.
    None,
}

/// Presentation of a multi-step form's progress, stored on `form`.
///
/// [`Self::default`] is the bar with its percentage shown, which is what a
/// `NULL` `form.progress_indicator` column decodes to. Note this is *not* the
/// pre-column behaviour — forms written before it rendered [`ProgressStyle::Steps`]
/// — a deliberate exception to the no-backfill convention used by
/// [`PostSubmissionAction`]: the bar is the intended default presentation, and
/// nothing needs migrating for a `NULL` to keep decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ProgressIndicator {
    pub style: ProgressStyle,
    /// Draw the numeric percentage next to the bar. Only read for
    /// [`ProgressStyle::Bar`].
    pub show_percent: bool,
}

impl Default for ProgressIndicator {
    fn default() -> Self {
        Self {
            style: ProgressStyle::Bar,
            show_percent: true,
        }
    }
}

/// Body of [`PostSubmissionAction::Message`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MessageAction {
    /// Confirmation copy. Plain text — newlines are preserved by the renderer,
    /// nothing is parsed as markup. `None` falls back to the renderer's
    /// built-in default message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Offer a button that resets the form in place for another submission.
    #[serde(default)]
    pub allow_resubmit: bool,
    /// Label for that button. `None` falls back to the renderer's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resubmit_label: Option<String>,
}

/// Body of [`PostSubmissionAction::Redirect`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RedirectAction {
    /// Absolute `http(s)` URL. Scheme-checked on every write by
    /// [`crate::forms::service::validate_post_submission_action`] — the embed
    /// runs inline in third-party pages, so a `javascript:` value here would be
    /// script execution on someone else's site.
    pub url: String,
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
    /// Ordered layout. When present it is the source of truth and
    /// `standard_fields`/`custom_fields` are derived from it; sending both is
    /// rejected. Absent means "derive a layout from the legacy pair".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Vec<FormElement>>,
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
    /// What the visitor sees once the form is submitted. Defaults to the
    /// built-in thank-you message.
    #[serde(default)]
    pub post_submission_action: PostSubmissionAction,
    /// How a multi-step form shows progress. Defaults to the bar with its
    /// percentage. Ignored by single-page forms.
    #[serde(default)]
    pub progress_indicator: ProgressIndicator,
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
    /// Ordered layout. Always populated — derived from the legacy pair for
    /// rows written before the `layout` column existed.
    pub layout: Vec<FormElement>,
    pub backends: Vec<BackendBinding>,
    pub tags: Vec<String>,
    /// Sales reps (by [`crate::reps`] id) this form offers.
    pub reps: Vec<i32>,
    /// Extra URL params captured as per-submission tags.
    pub source_params: Vec<SourceParam>,
    /// What the visitor sees once the form is submitted.
    pub post_submission_action: PostSubmissionAction,
    /// How a multi-step form shows progress.
    pub progress_indicator: ProgressIndicator,
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
    /// Replaces the whole layout. Mutually exclusive with
    /// `standard_fields`/`custom_fields` — a request carrying both is a 400,
    /// because the layout fully determines those two and silently picking a
    /// winner would be undetectable by the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Vec<FormElement>>,
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
    /// `None` leaves the post-submission action untouched. Sending the default
    /// action (a `message` with no overrides) resets the column to `NULL`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_submission_action: Option<PostSubmissionAction>,
    /// `None` leaves the progress indicator untouched. Sending the default
    /// (a bar with its percentage) resets the column to `NULL`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_indicator: Option<ProgressIndicator>,
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
    /// Retained for embed bundles cached on host pages before `layout`
    /// existed. Always a faithful projection of `layout`.
    pub standard_fields: StandardFieldsConfig,
    /// See `standard_fields`.
    pub custom_fields: Vec<CustomField>,
    /// Ordered layout — what current renderers consume.
    pub layout: Vec<FormElement>,
    pub backends: Vec<BackendBinding>,
    /// What the renderer does once a submission is accepted. Always populated;
    /// a bundle too old to know the field just ignores it.
    pub post_submission_action: PostSubmissionAction,
    /// How the renderer shows progress through a multi-step form. Always
    /// populated; a bundle too old to know the field just ignores it and keeps
    /// drawing the `Step N of M` line.
    pub progress_indicator: ProgressIndicator,
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
