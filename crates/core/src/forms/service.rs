//! Form persistence + validation.
//!
//! All functions take `&impl ConnectionTrait` so callers can pass either
//! a `DatabaseConnection` or a `DatabaseTransaction`.

use std::collections::{HashMap, HashSet};

use anyhow::anyhow;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect,
};

use super::{
    BackendBinding, ConditionOp, CustomField, CustomFieldType, FormDto, FormElement, FormList,
    FormSelectOption, ListQuery, MessageAction, NewForm, PostSubmissionAction, ProgressIndicator,
    PublicFormDto, RedirectAction, STANDARD_FIELD_KEYS, SourceParam, StandardElement,
    StandardFieldsConfig, StandardInputVariant, UpdateForm, VisibilityRule, default_backends,
};
use crate::backend::BackendRegistry;
use crate::error::{CoreError, CoreResult};
use crate::metadata::service as metadata_service;
use crate::reps::service as reps_service;

const MAX_NAME_LEN: usize = 200;
const MAX_SLUG_LEN: usize = 100;
const MAX_LABEL_LEN: usize = 200;
const MAX_KEY_LEN: usize = 64;
const MAX_CUSTOM_FIELDS: usize = 100;
const MAX_LAYOUT_ELEMENTS: usize = 300;
const MAX_PARAGRAPH_LEN: usize = 2000;
const MAX_PAGES: usize = 20;
/// Conditions in one visibility rule. Generous for real forms, low enough that
/// the per-element evaluation stays trivially cheap on the submission path.
const MAX_CONDITIONS: usize = 10;
const MAX_TAG_LEN: usize = 255;
const MAX_SOURCE_PARAMS: usize = 50;
const MAX_PARAM_LEN: usize = 64;
const MAX_TAG_PREFIX_LEN: usize = 64;
const MAX_POST_SUBMISSION_MESSAGE_LEN: usize = 2000;
const MAX_REDIRECT_URL_LEN: usize = 2048;
const DEFAULT_LIST_LIMIT: u32 = 50;
const MAX_LIST_LIMIT: u32 = 200;

/// Reserved query param resolved to a sales rep, not captured as a tag. Source
/// params may not reuse this name.
pub const REP_PARAM: &str = "rep";

pub fn validate_name(name: &str) -> CoreResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_NAME_LEN {
        return Err(CoreError::BadRequest(format!(
            "name must be 1..={MAX_NAME_LEN} characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// Slugs are lowercase ASCII letters/digits/hyphens. Hyphens may not lead,
/// trail, or repeat. Mirrors the URL-safety constraint of the public embed
/// route (slug is the natural id consumer-facing).
pub fn validate_slug(slug: &str) -> CoreResult<String> {
    let s = slug.trim().to_string();
    if s.is_empty() || s.len() > MAX_SLUG_LEN {
        return Err(CoreError::BadRequest(format!(
            "slug must be 1..={MAX_SLUG_LEN} characters"
        )));
    }
    let bytes = s.as_bytes();
    if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
        return Err(CoreError::BadRequest(
            "slug must not start or end with '-'".into(),
        ));
    }
    let mut prev_hyphen = false;
    for &b in bytes {
        let ok = matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-');
        if !ok {
            return Err(CoreError::BadRequest(
                "slug must contain only lowercase letters, digits, and hyphens".into(),
            ));
        }
        if b == b'-' && prev_hyphen {
            return Err(CoreError::BadRequest(
                "slug must not contain consecutive hyphens".into(),
            ));
        }
        prev_hyphen = b == b'-';
    }
    Ok(s)
}

/// Best-effort slugification: lowercase, ASCII-only, replace runs of
/// non-alphanumerics with single hyphens, trim leading/trailing hyphens,
/// truncate to `MAX_SLUG_LEN`. The result is then re-validated.
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_hyphen = true;
    for c in input.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_hyphen = false;
        } else if !prev_hyphen {
            out.push('-');
            prev_hyphen = true;
        }
    }
    if out.ends_with('-') {
        out.pop();
    }
    if out.len() > MAX_SLUG_LEN {
        out.truncate(MAX_SLUG_LEN);
        if out.ends_with('-') {
            out.pop();
        }
    }
    out
}

/// Custom field keys are intentionally format-agnostic. The key is used
/// verbatim both as the submission JSON key and as the lookup key when a
/// backend maps it onto a destination field — e.g. a GoHighLevel custom-field
/// "unique key" (`contact.how_did_you_hear`) or a raw field id (`aBc123`).
/// Forcing snake_case here would make those backends impossible to target, so
/// we accept any key the destination needs and only reject the two things that
/// would actually break the wire shape: emptiness/oversize, and whitespace or
/// control characters (which can't appear in a well-formed JSON object key the
/// admin can reason about).
fn validate_custom_field_key(key: &str) -> CoreResult<()> {
    let count = key.chars().count();
    if count == 0 || count > MAX_KEY_LEN {
        return Err(CoreError::BadRequest(format!(
            "custom field key must be 1..={MAX_KEY_LEN} characters"
        )));
    }
    if key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(CoreError::BadRequest(
            "custom field key must not contain whitespace or control characters".into(),
        ));
    }
    Ok(())
}

pub fn validate_custom_fields(fields: &[CustomField]) -> CoreResult<()> {
    if fields.len() > MAX_CUSTOM_FIELDS {
        return Err(CoreError::BadRequest(format!(
            "no more than {MAX_CUSTOM_FIELDS} custom fields allowed"
        )));
    }
    let mut seen_keys: HashSet<&str> = HashSet::with_capacity(fields.len());
    for f in fields {
        validate_custom_field_key(&f.key)?;
        if !seen_keys.insert(f.key.as_str()) {
            return Err(CoreError::BadRequest(format!(
                "duplicate custom field key: {}",
                f.key
            )));
        }
        // A standard-field key would collide with the same column in the
        // submission shape; reject the overlap up front.
        if STANDARD_FIELD_KEYS.contains(&f.key.as_str()) {
            return Err(CoreError::BadRequest(format!(
                "custom field key '{}' collides with a standard field",
                f.key
            )));
        }
        let label = f.label.trim();
        if label.is_empty() || label.len() > MAX_LABEL_LEN {
            return Err(CoreError::BadRequest(format!(
                "custom field '{}' label must be 1..={MAX_LABEL_LEN} characters",
                f.key
            )));
        }
        if let Some(options) = f.kind.options() {
            if options.is_empty() {
                return Err(CoreError::BadRequest(format!(
                    "custom field '{}' offers a choice but has no options",
                    f.key
                )));
            }
            let mut seen_opts: HashSet<&str> = HashSet::with_capacity(options.len());
            for opt in options {
                let t = opt.trim();
                if t.is_empty() {
                    return Err(CoreError::BadRequest(format!(
                        "custom field '{}' has a blank option",
                        f.key
                    )));
                }
                if !seen_opts.insert(t) {
                    return Err(CoreError::BadRequest(format!(
                        "custom field '{}' has duplicate option '{}'",
                        f.key, t
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Tags are permissive strings (GHL accepts spaces, emoji, etc.).
/// Validation: trim whitespace, reject empty/blank strings, reject any tag
/// longer than `MAX_TAG_LEN`, deduplicate case-sensitively. No count limit.
pub fn validate_tags(tags: &[String]) -> CoreResult<Vec<String>> {
    let mut out: Vec<String> = Vec::with_capacity(tags.len());
    for t in tags {
        let trimmed = t.trim();
        if trimmed.is_empty() {
            return Err(CoreError::BadRequest("tag must not be blank".into()));
        }
        if trimmed.chars().count() > MAX_TAG_LEN {
            return Err(CoreError::BadRequest(format!(
                "tag exceeds {MAX_TAG_LEN} characters"
            )));
        }
        // Case-sensitive dedup to mirror GHL behaviour.
        if !out.iter().any(|existing| existing == trimmed) {
            out.push(trimmed.to_string());
        }
    }
    Ok(out)
}

/// Parse the `tags` JSON column from a form model. `NULL` → empty vec.
pub fn tags_from_model(m: &entity::form::Model) -> CoreResult<Vec<String>> {
    match &m.tags {
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| CoreError::Internal(anyhow!("failed to parse tags json: {e}"))),
        None => Ok(Vec::new()),
    }
}

/// Validate + normalize the rep ids a form offers: dedup (order-preserving) and
/// reject any id that doesn't resolve to a [`crate::reps`] row.
pub async fn validate_reps<C: ConnectionTrait>(conn: &C, ids: &[i32]) -> CoreResult<Vec<i32>> {
    let mut seen = HashSet::with_capacity(ids.len());
    let mut deduped: Vec<i32> = Vec::with_capacity(ids.len());
    for &id in ids {
        if seen.insert(id) {
            deduped.push(id);
        }
    }
    if deduped.is_empty() {
        return Ok(deduped);
    }
    let existing: HashSet<i32> = reps_service::existing_ids(conn, &deduped)
        .await?
        .into_iter()
        .collect();
    for id in &deduped {
        if !existing.contains(id) {
            return Err(CoreError::BadRequest(format!("unknown sales rep: {id}")));
        }
    }
    Ok(deduped)
}

/// Validate + normalize source params: trim, reject blanks, the reserved `rep`
/// name, duplicates, oversize names/prefixes, and an excess count.
pub fn validate_source_params(params: &[SourceParam]) -> CoreResult<Vec<SourceParam>> {
    if params.len() > MAX_SOURCE_PARAMS {
        return Err(CoreError::BadRequest(format!(
            "no more than {MAX_SOURCE_PARAMS} source params allowed"
        )));
    }
    let mut seen: HashSet<String> = HashSet::with_capacity(params.len());
    let mut out: Vec<SourceParam> = Vec::with_capacity(params.len());
    for p in params {
        let param = p.param.trim();
        if param.is_empty() {
            return Err(CoreError::BadRequest("source param name must not be blank".into()));
        }
        if param.chars().count() > MAX_PARAM_LEN {
            return Err(CoreError::BadRequest(format!(
                "source param name exceeds {MAX_PARAM_LEN} characters"
            )));
        }
        if param.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(CoreError::BadRequest(
                "source param name must not contain whitespace or control characters".into(),
            ));
        }
        if param == REP_PARAM {
            return Err(CoreError::BadRequest(format!(
                "'{REP_PARAM}' is reserved and can't be a source param"
            )));
        }
        if !seen.insert(param.to_string()) {
            return Err(CoreError::BadRequest(format!(
                "duplicate source param: {param}"
            )));
        }
        let tag_prefix = match &p.tag_prefix {
            Some(s) => {
                let t = s.trim();
                if t.is_empty() {
                    None
                } else if t.chars().count() > MAX_TAG_PREFIX_LEN {
                    return Err(CoreError::BadRequest(format!(
                        "source param tag prefix exceeds {MAX_TAG_PREFIX_LEN} characters"
                    )));
                } else {
                    Some(t.to_string())
                }
            }
            None => None,
        };
        out.push(SourceParam {
            param: param.to_string(),
            tag_prefix,
        });
    }
    Ok(out)
}

/// Parse the `reps` JSON column from a form model. `NULL` → empty vec.
pub fn reps_from_model(m: &entity::form::Model) -> CoreResult<Vec<i32>> {
    match &m.reps {
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| CoreError::Internal(anyhow!("failed to parse reps json: {e}"))),
        None => Ok(Vec::new()),
    }
}

/// Parse the `source_params` JSON column from a form model. `NULL` → empty vec.
pub fn source_params_from_model(m: &entity::form::Model) -> CoreResult<Vec<SourceParam>> {
    match &m.source_params {
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| CoreError::Internal(anyhow!("failed to parse source_params json: {e}"))),
        None => Ok(Vec::new()),
    }
}

/// Validate + normalize a post-submission action.
///
/// Message copy and the resubmit label are trimmed, and an empty one becomes
/// `None` so a cleared textarea falls back to the renderer's built-in default
/// rather than rendering a blank confirmation panel.
pub fn validate_post_submission_action(
    action: &PostSubmissionAction,
) -> CoreResult<PostSubmissionAction> {
    match action {
        PostSubmissionAction::Message(m) => Ok(PostSubmissionAction::Message(MessageAction {
            message: trimmed_within(
                m.message.as_deref(),
                MAX_POST_SUBMISSION_MESSAGE_LEN,
                "thank-you message",
            )?,
            allow_resubmit: m.allow_resubmit,
            resubmit_label: trimmed_within(
                m.resubmit_label.as_deref(),
                MAX_LABEL_LEN,
                "submit-another button label",
            )?,
        })),
        PostSubmissionAction::Redirect(r) => Ok(PostSubmissionAction::Redirect(RedirectAction {
            url: validate_redirect_url(&r.url)?,
        })),
    }
}

/// Trim an optional string, mapping empty to `None` and rejecting oversize.
fn trimmed_within(value: Option<&str>, max: usize, label: &str) -> CoreResult<Option<String>> {
    let Some(raw) = value else { return Ok(None) };
    let t = raw.trim();
    if t.is_empty() {
        return Ok(None);
    }
    if t.chars().count() > max {
        return Err(CoreError::BadRequest(format!(
            "{label} exceeds {max} characters"
        )));
    }
    Ok(Some(t.to_string()))
}

/// Require an absolute `http(s)` URL.
///
/// This is the only thing between a form admin and script execution on someone
/// else's site: the embed SDK runs inline in the host document, so the renderer
/// hands this value straight to `window.location.assign`. A bad scheme is a
/// hard 400, never a silent normalization.
///
/// Deliberately hand-rolled rather than pulling in a URL parser — we only need
/// to *reject*, and the checks below are stricter than a parser's would be.
fn validate_redirect_url(raw: &str) -> CoreResult<String> {
    let url = raw.trim();
    if url.is_empty() {
        return Err(CoreError::BadRequest("redirect url is required".into()));
    }
    if url.len() > MAX_REDIRECT_URL_LEN {
        return Err(CoreError::BadRequest(format!(
            "redirect url exceeds {MAX_REDIRECT_URL_LEN} characters"
        )));
    }
    // Whitespace and control characters are stripped or collapsed by browsers
    // during URL parsing, so `java\nscript:...` would pass a naive scheme check
    // and then re-form into a live scheme. Reject them outright, before it.
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(CoreError::BadRequest(
            "redirect url must not contain whitespace or control characters".into(),
        ));
    }
    let lower = url.to_ascii_lowercase();
    // Also rejects scheme-relative "//evil.example", which would otherwise
    // inherit the host page's scheme and navigate off-site.
    let rest = match lower.strip_prefix("http://") {
        Some(r) => r,
        None => lower.strip_prefix("https://").ok_or_else(|| {
            CoreError::BadRequest("redirect url must be an absolute http(s) URL".into())
        })?,
    };
    // An empty authority ("https:///path") resolves against the current origin.
    if rest.is_empty() || rest.starts_with(['/', '?', '#']) {
        return Err(CoreError::BadRequest(
            "redirect url must include a host".into(),
        ));
    }
    Ok(url.to_string())
}

/// Parse the `post_submission_action` JSON column from a form model. `NULL` →
/// the default (built-in thank-you message), i.e. pre-feature behaviour.
pub fn post_submission_action_from_model(
    m: &entity::form::Model,
) -> CoreResult<PostSubmissionAction> {
    match &m.post_submission_action {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| {
            CoreError::Internal(anyhow!("failed to parse post_submission_action json: {e}"))
        }),
        None => Ok(PostSubmissionAction::default()),
    }
}

/// Parse the `progress_indicator` JSON column from a form model. `NULL` → the
/// default (a bar showing its percentage).
pub fn progress_indicator_from_model(m: &entity::form::Model) -> CoreResult<ProgressIndicator> {
    match &m.progress_indicator {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| {
            CoreError::Internal(anyhow!("failed to parse progress_indicator json: {e}"))
        }),
        None => Ok(ProgressIndicator::default()),
    }
}

/// Reject empty bindings, duplicates, kinds the registry doesn't know about,
/// or instance ids that don't resolve to a `backend_instance` row of the
/// matching kind. Configurable kinds *must* carry an `instance_id`; static
/// kinds *must not*.
pub async fn validate_backends<C: ConnectionTrait>(
    conn: &C,
    bindings: &[BackendBinding],
    registry: &BackendRegistry,
) -> CoreResult<()> {
    if bindings.is_empty() {
        return Err(CoreError::BadRequest(
            "form must have at least one backend".into(),
        ));
    }
    let mut seen: HashSet<(String, Option<i32>)> = HashSet::with_capacity(bindings.len());
    for b in bindings {
        let kind = b.kind.trim();
        if kind.is_empty() {
            return Err(CoreError::BadRequest("backend kind is empty".into()));
        }
        let key = (kind.to_string(), b.instance_id);
        if !seen.insert(key) {
            return Err(CoreError::BadRequest(format!(
                "duplicate backend binding: {kind}{}",
                b.instance_id
                    .map(|id| format!(":{id}"))
                    .unwrap_or_default()
            )));
        }
        if !registry.knows(kind) {
            return Err(CoreError::BadRequest(format!("unknown backend: {kind}")));
        }
        let configurable = registry.is_configurable(kind);
        match (configurable, b.instance_id) {
            (true, None) => {
                return Err(CoreError::BadRequest(format!(
                    "backend '{kind}' requires an instance_id"
                )));
            }
            (false, Some(_)) => {
                return Err(CoreError::BadRequest(format!(
                    "backend '{kind}' is not configurable; drop instance_id"
                )));
            }
            (true, Some(instance_id)) => {
                let inst = entity::backend_instance::Entity::find_by_id(instance_id)
                    .one(conn)
                    .await?;
                let Some(inst) = inst else {
                    return Err(CoreError::BadRequest(format!(
                        "backend instance {instance_id} not found"
                    )));
                };
                if inst.kind != kind {
                    return Err(CoreError::BadRequest(format!(
                        "backend instance {instance_id} is kind '{}', expected '{kind}'",
                        inst.kind
                    )));
                }
            }
            (false, None) => {}
        }
    }
    Ok(())
}

fn parse_backends(value: &sea_orm::JsonValue) -> CoreResult<Vec<BackendBinding>> {
    serde_json::from_value(value.clone())
        .map_err(|e| CoreError::Internal(anyhow!("failed to parse backends json: {e}")))
}

/// Canonicalise a standard-field config before it is written.
///
/// A disabled field has no observable `required`/`label`, so leaving stale
/// values on it means two configs that render and validate identically can
/// still differ byte-for-byte. That matters once `layout` lands: the layout has
/// no element for a disabled key, so projecting a layout back to a
/// `StandardFieldsConfig` can only ever produce the zeroed form. Normalising on
/// write keeps legacy -> layout -> legacy an exact identity instead of a
/// silent regression on the first save.
fn normalize_standard_fields(mut cfg: StandardFieldsConfig) -> StandardFieldsConfig {
    for f in cfg.iter_mut() {
        if !f.enabled {
            f.required = false;
            f.label = None;
        } else if let Some(label) = &f.label {
            let trimmed = label.trim();
            f.label = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }
    }
    cfg
}

fn normalize_custom_fields(mut fields: Vec<CustomField>) -> Vec<CustomField> {
    fields.sort_by_key(|f| f.position);
    for (idx, f) in fields.iter_mut().enumerate() {
        f.position = idx as i32;
        f.label = f.label.trim().to_string();
        if let CustomFieldType::Select { options } = &mut f.kind {
            for o in options.iter_mut() {
                *o = o.trim().to_string();
            }
        }
    }
    fields
}

/// Derive a layout from the legacy column pair.
///
/// The order here is the compat guarantee: enabled standard fields in
/// `STANDARD_FIELD_KEYS` order, then custom fields by `position`. That is
/// exactly what the pre-layout renderer drew (`Form.tsx` rendered
/// `enabledStandard` then `orderedCustom`), so a form that has never been
/// touched by the builder renders identically before and after this change.
pub fn layout_from_legacy(sf: &StandardFieldsConfig, cf: &[CustomField]) -> Vec<FormElement> {
    let mut out: Vec<FormElement> = Vec::with_capacity(cf.len() + STANDARD_FIELD_KEYS.len());
    for (key, cfg) in sf.iter() {
        if cfg.enabled {
            out.push(FormElement::Standard(StandardElement::from_legacy(key, cfg)));
        }
    }
    let mut customs = cf.to_vec();
    customs.sort_by_key(|f| f.position);
    out.extend(customs.into_iter().map(FormElement::Custom));
    out
}

/// Project a layout back onto the legacy column pair.
///
/// Lossy by design, and only in the downlevel direction: decoration elements
/// disappear, a standard element's `placeholder`/`help_text`/`width`/
/// `default_value`/`input_override` have nowhere to live in
/// [`StandardFieldConfig`], and interleaving collapses to standards-then-customs.
/// None of that affects what the server validates or what a backend receives —
/// it only changes what a pre-layout embed bundle draws.
pub fn legacy_from_layout(layout: &[FormElement]) -> (StandardFieldsConfig, Vec<CustomField>) {
    let mut sf = StandardFieldsConfig::all_disabled();
    let mut cf: Vec<CustomField> = Vec::new();
    for el in layout {
        match el {
            FormElement::Standard(s) => {
                if let Some(slot) = sf.get_mut(&s.key) {
                    slot.enabled = true;
                    slot.required = s.required;
                    slot.label = s.label.clone();
                }
            }
            FormElement::Custom(c) => {
                let mut c = c.clone();
                c.position = cf.len() as i32;
                cf.push(c);
            }
            _ => {}
        }
    }
    (sf, cf)
}

/// Fold a legacy write into an existing layout, preserving builder-chosen order.
///
/// Callers that only speak `standard_fields`/`custom_fields` must not silently
/// flatten a hand-ordered layout, so this updates in place rather than
/// rebuilding. Both inputs are `Option` because `update_form` sets the two
/// columns independently — a PATCH touching one must leave the other alone.
pub fn merge_legacy_into_layout(
    existing: &[FormElement],
    incoming_sf: Option<&StandardFieldsConfig>,
    incoming_cf: Option<&[CustomField]>,
) -> Vec<FormElement> {
    if incoming_sf.is_none() && incoming_cf.is_none() {
        return existing.to_vec();
    }

    let mut seen_standard: HashSet<String> = HashSet::new();
    let mut seen_custom: HashSet<String> = HashSet::new();
    let mut out: Vec<FormElement> = Vec::with_capacity(existing.len());

    // Pass 1 — update or drop in place. Nothing moves.
    for el in existing {
        match el {
            FormElement::Standard(s) => match incoming_sf {
                Some(sf) => {
                    let Some(cfg) = sf.get(&s.key) else { continue };
                    if !cfg.enabled {
                        continue;
                    }
                    seen_standard.insert(s.key.clone());
                    // Carry forward the layout-only extras the legacy payload
                    // cannot express, so an old client toggling `required`
                    // doesn't wipe a placeholder the builder set.
                    out.push(FormElement::Standard(StandardElement {
                        required: cfg.required,
                        label: cfg.label.clone(),
                        ..s.clone()
                    }));
                }
                None => out.push(el.clone()),
            },
            FormElement::Custom(c) => match incoming_cf {
                Some(cfs) => match cfs.iter().find(|n| n.key == c.key) {
                    Some(n) => {
                        seen_custom.insert(c.key.clone());
                        // Carry the visibility rule forward when the incoming
                        // field doesn't state one, for the same reason the
                        // standard branch above carries a placeholder: a client
                        // that builds `custom_fields` from scratch has no
                        // vocabulary for rules and must not silently delete one.
                        // (`Option` can't tell "omitted" from "explicit null",
                        // so carry-when-absent is the only available semantics —
                        // the same trade the standard branch makes for `label`.)
                        let mut n = n.clone();
                        if n.visible_when.is_none() {
                            n.visible_when = c.visible_when.clone();
                        }
                        out.push(FormElement::Custom(n));
                    }
                    None => continue,
                },
                None => out.push(el.clone()),
            },
            other => out.push(other.clone()),
        }
    }

    // Pass 2 — newly enabled standard fields land at their catalogue position.
    //
    // A client that speaks only `standard_fields` has no ordering vocabulary,
    // and the only order it has ever observed is the catalogue's. Appending at
    // the end would surface "enable phone" after the message box and every
    // custom field, which such a client has no way to correct.
    if let Some(sf) = incoming_sf {
        for (key, cfg) in sf.iter() {
            if !cfg.enabled || seen_standard.contains(key) {
                continue;
            }
            let el = FormElement::Standard(StandardElement::from_legacy(key, cfg));
            let cat = |k: &str| STANDARD_FIELD_KEYS.iter().position(|c| *c == k);
            let my_pos = cat(key);
            let successor = out.iter().position(|e| match e {
                FormElement::Standard(s) => cat(&s.key) > my_pos,
                _ => false,
            });
            match successor {
                Some(i) => out.insert(i, el),
                None => {
                    // After the last standard field, but never pushed past a
                    // page break into a later step.
                    let last_std = out.iter().rposition(|e| matches!(e, FormElement::Standard(_)));
                    match last_std {
                        Some(i) => {
                            let brk = out[i + 1..]
                                .iter()
                                .position(|e| matches!(e, FormElement::PageBreak(_)))
                                .map(|b| i + 1 + b);
                            out.insert(brk.unwrap_or(i + 1), el);
                        }
                        None => out.push(el),
                    }
                }
            }
        }
    }

    // Pass 3 — brand-new custom fields append in incoming order.
    if let Some(cfs) = incoming_cf {
        let mut fresh: Vec<&CustomField> =
            cfs.iter().filter(|n| !seen_custom.contains(&n.key)).collect();
        fresh.sort_by_key(|f| f.position);
        out.extend(fresh.into_iter().cloned().map(FormElement::Custom));
    }

    // Pass 4 — honour a reorder expressed through `position`, without moving
    // any non-custom element. Refill the custom slots in the incoming order.
    if let Some(cfs) = incoming_cf {
        let slots: Vec<usize> = out
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, FormElement::Custom(_)))
            .map(|(i, _)| i)
            .collect();
        let mut ordered: Vec<CustomField> = out
            .iter()
            .filter_map(|e| match e {
                FormElement::Custom(c) => Some(c.clone()),
                _ => None,
            })
            .collect();
        let rank = |k: &str| cfs.iter().position(|n| n.key == k).unwrap_or(usize::MAX);
        ordered.sort_by_key(|c| rank(&c.key));
        for (slot, c) in slots.into_iter().zip(ordered) {
            out[slot] = FormElement::Custom(c);
        }
    }

    out
}

/// Trim copy, clamp heading levels, and renumber custom positions to match
/// layout order. Mirrors what [`normalize_custom_fields`] does for the legacy
/// column.
fn normalize_layout(mut layout: Vec<FormElement>) -> Vec<FormElement> {
    let mut custom_idx = 0i32;
    for el in layout.iter_mut() {
        match el {
            FormElement::Standard(s) => {
                s.label = s.label.as_ref().map(|l| l.trim().to_string()).filter(|l| !l.is_empty());
            }
            FormElement::Custom(c) => {
                c.position = custom_idx;
                custom_idx += 1;
                c.label = c.label.trim().to_string();
                if let Some(options) = c.kind.options_mut() {
                    for o in options.iter_mut() {
                        *o = o.trim().to_string();
                    }
                }
            }
            FormElement::Heading(h) => {
                h.text = h.text.trim().to_string();
                h.level = h.level.clamp(1, 6);
            }
            FormElement::Paragraph(p) => {
                p.text = p.text.trim().to_string();
            }
            FormElement::PageBreak(b) => {
                b.title = b.title.as_ref().map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
            }
            FormElement::Divider => {}
        }
        // Trim every rule the element carries, and collapse one left with no
        // conditions back to "unconditional" — an empty rule would otherwise
        // read as `all of nothing`, i.e. always visible, which is the same
        // behaviour spelled in a way that survives a round trip badly.
        if let Some(slot @ Some(_)) = el.visible_when_slot() {
            if let Some(rule) = slot.as_mut() {
                for c in rule.conditions.iter_mut() {
                    c.field = c.field.trim().to_string();
                    c.value = c.value.as_ref().map(|v| v.trim().to_string());
                }
                if rule.conditions.is_empty() {
                    *slot = None;
                }
            }
        }
    }
    layout
}

/// Drop rules a *legacy* write has stranded.
///
/// A caller that speaks only `standard_fields`/`custom_fields` has no vocabulary
/// for conditions, yet its writes move elements around: disabling a standard
/// field removes it, enabling one inserts it at its catalogue position, and a
/// `position` reorder can move a controller behind its dependent. Any of those
/// can leave a rule pointing at a key that is gone or no longer earlier — and
/// rejecting the request would 400 a client for a rule it never sent and cannot
/// see. So the legacy paths repair instead: the element stays, unconditional.
///
/// Explicit `layout` writes never go through this. There the caller *did* send
/// the rule, so `validate_layout` tells them what's wrong.
fn strip_dangling_rules(mut layout: Vec<FormElement>) -> Vec<FormElement> {
    let mut seen: HashSet<String> = HashSet::new();
    for el in layout.iter_mut() {
        let key = el.field_key().map(str::to_string);
        if let Some(slot) = el.visible_when_slot() {
            let dangling = slot
                .as_ref()
                .is_some_and(|r| r.conditions.iter().any(|c| !seen.contains(&c.field)));
            if dangling {
                *slot = None;
            }
        }
        if let Some(k) = key {
            seen.insert(k);
        }
    }
    layout
}

/// Validate one element's visibility rule against the fields that precede it.
///
/// `seen` holds every field key already passed, mapped to whether that field is
/// a checkbox. Requiring a condition's target to be in `seen` — i.e. strictly
/// earlier in the layout — is what rejects unknown keys, self-references and
/// cycles all at once, and it is the invariant the single-pass evaluators in
/// `forms::visibility` and `visibility.ts` are built on.
fn validate_visible_when(
    rule: &VisibilityRule,
    owner: &str,
    seen: &HashMap<String, bool>,
) -> CoreResult<()> {
    if rule.conditions.is_empty() {
        return Err(CoreError::BadRequest(format!(
            "'{owner}' has a visibility rule with no conditions"
        )));
    }
    if rule.conditions.len() > MAX_CONDITIONS {
        return Err(CoreError::BadRequest(format!(
            "'{owner}' has more than {MAX_CONDITIONS} conditions"
        )));
    }
    for c in &rule.conditions {
        let Some(&is_checkbox) = seen.get(&c.field) else {
            return Err(CoreError::BadRequest(format!(
                "'{owner}' can only depend on a field that appears earlier in the form; \
                 '{}' does not",
                c.field
            )));
        };
        match (c.op.takes_value(), &c.value) {
            (true, None) => {
                return Err(CoreError::BadRequest(format!(
                    "'{owner}' has a condition on '{}' that needs a value",
                    c.field
                )));
            }
            (false, Some(_)) => {
                return Err(CoreError::BadRequest(format!(
                    "'{owner}' has a condition on '{}' that takes no value",
                    c.field
                )));
            }
            _ => {}
        }
        if let Some(v) = &c.value
            && v.chars().count() > MAX_LABEL_LEN
        {
            return Err(CoreError::BadRequest(format!(
                "'{owner}' has a condition value longer than {MAX_LABEL_LEN} characters"
            )));
        }
        if matches!(c.op, ConditionOp::IsChecked | ConditionOp::IsNotChecked) && !is_checkbox {
            return Err(CoreError::BadRequest(format!(
                "'{owner}' tests whether '{}' is checked, but it is not a checkbox",
                c.field
            )));
        }
    }
    Ok(())
}

pub fn validate_layout(layout: &[FormElement]) -> CoreResult<()> {
    if layout.len() > MAX_LAYOUT_ELEMENTS {
        return Err(CoreError::BadRequest(format!(
            "no more than {MAX_LAYOUT_ELEMENTS} layout elements allowed"
        )));
    }

    let mut seen_standard: HashSet<&str> = HashSet::new();
    // Field keys already passed, mapped to "is a checkbox" — the two things a
    // visibility rule needs to know about a candidate controller.
    let mut seen_fields: HashMap<String, bool> = HashMap::new();
    let mut pages = 1usize;
    for (i, el) in layout.iter().enumerate() {
        // Rules are checked before the element records its own key, so a
        // self-reference falls out as "does not appear earlier".
        if let Some(rule) = el.visible_when() {
            let owner = el.field_key().unwrap_or(match el {
                FormElement::Heading(_) => "heading",
                FormElement::Paragraph(_) => "paragraph",
                _ => "element",
            });
            validate_visible_when(rule, owner, &seen_fields)?;
        }
        match el {
            FormElement::Standard(s) => {
                if !STANDARD_FIELD_KEYS.contains(&s.key.as_str()) {
                    return Err(CoreError::BadRequest(format!(
                        "unknown standard field key: {}",
                        s.key
                    )));
                }
                if !seen_standard.insert(s.key.as_str()) {
                    return Err(CoreError::BadRequest(format!(
                        "duplicate standard field: {}",
                        s.key
                    )));
                }
                if let Some(l) = &s.label {
                    if l.chars().count() > MAX_LABEL_LEN {
                        return Err(CoreError::BadRequest(format!(
                            "standard field '{}' label must be at most {MAX_LABEL_LEN} characters",
                            s.key
                        )));
                    }
                }
                if s.input_override == Some(StandardInputVariant::Select) && s.key != "country" {
                    return Err(CoreError::BadRequest(format!(
                        "standard field '{}' has no select variant",
                        s.key
                    )));
                }
            }
            FormElement::Heading(h) => {
                if h.text.is_empty() || h.text.chars().count() > MAX_LABEL_LEN {
                    return Err(CoreError::BadRequest(format!(
                        "heading text must be 1..={MAX_LABEL_LEN} characters"
                    )));
                }
                if !(1..=6).contains(&h.level) {
                    return Err(CoreError::BadRequest(
                        "heading level must be between 1 and 6".into(),
                    ));
                }
            }
            FormElement::Paragraph(p) => {
                if p.text.is_empty() || p.text.chars().count() > MAX_PARAGRAPH_LEN {
                    return Err(CoreError::BadRequest(format!(
                        "paragraph text must be 1..={MAX_PARAGRAPH_LEN} characters"
                    )));
                }
            }
            FormElement::PageBreak(_) => {
                // A break before any content, or two in a row, would render an
                // empty step with a Next button and nothing to fill in.
                if i == 0 {
                    return Err(CoreError::BadRequest(
                        "a form cannot start with a page break".into(),
                    ));
                }
                if matches!(layout.get(i.wrapping_sub(1)), Some(FormElement::PageBreak(_))) {
                    return Err(CoreError::BadRequest("consecutive page breaks".into()));
                }
                if i == layout.len() - 1 {
                    return Err(CoreError::BadRequest(
                        "a form cannot end with a page break".into(),
                    ));
                }
                pages += 1;
                if pages > MAX_PAGES {
                    return Err(CoreError::BadRequest(format!(
                        "no more than {MAX_PAGES} pages allowed"
                    )));
                }
            }
            FormElement::Custom(_) | FormElement::Divider => {}
        }
        if let Some(key) = el.field_key() {
            // A standard field is never a checkbox, so `false` is right for it.
            let is_checkbox = matches!(el, FormElement::Custom(c) if c.kind.is_checkbox());
            seen_fields.insert(key.to_string(), is_checkbox);
        }
    }

    // Delegate every custom-field rule (key charset, dupes, collision with a
    // standard key, select options) rather than restating them here.
    let customs: Vec<CustomField> = layout
        .iter()
        .filter_map(|e| match e {
            FormElement::Custom(c) => Some(c.clone()),
            _ => None,
        })
        .collect();
    validate_custom_fields(&customs)
}

pub fn parse_layout(value: &sea_orm::JsonValue) -> CoreResult<Vec<FormElement>> {
    serde_json::from_value(value.clone())
        .map_err(|e| CoreError::Internal(anyhow!("failed to parse layout json: {e}")))
}

/// The layout for a row, materialising one from the legacy pair when the
/// column is `NULL` (written before `layout` existed).
pub fn layout_from_model(m: &entity::form::Model) -> CoreResult<Vec<FormElement>> {
    match &m.layout {
        Some(v) => parse_layout(v),
        None => Ok(layout_from_legacy(
            &parse_standard_fields(&m.standard_fields)?,
            &parse_custom_fields(&m.custom_fields)?,
        )),
    }
}

/// Reject a request that sets both the layout and the legacy pair it derives.
///
/// Not merely redundant: the layout fully determines the projection, so
/// honouring both means discarding one of two stated intents. "Layout wins"
/// would be undetectable by the caller, since the response echoes a projection
/// that looks plausibly like what it sent.
fn reject_conflicting_field_inputs(
    layout: bool,
    standard_fields: bool,
    custom_fields: bool,
) -> CoreResult<()> {
    if layout && (standard_fields || custom_fields) {
        return Err(CoreError::BadRequest(
            "send either `layout` or `standard_fields`/`custom_fields`, not both".into(),
        ));
    }
    Ok(())
}

pub fn parse_standard_fields(value: &sea_orm::JsonValue) -> CoreResult<StandardFieldsConfig> {
    serde_json::from_value(value.clone())
        .map_err(|e| CoreError::Internal(anyhow!("failed to parse standard_fields json: {e}")))
}

pub fn parse_custom_fields(value: &sea_orm::JsonValue) -> CoreResult<Vec<CustomField>> {
    serde_json::from_value(value.clone())
        .map_err(|e| CoreError::Internal(anyhow!("failed to parse custom_fields json: {e}")))
}

fn json_or_internal<T: serde::Serialize>(t: &T) -> CoreResult<sea_orm::JsonValue> {
    serde_json::to_value(t).map_err(|e| CoreError::Internal(anyhow!("json serialize failed: {e}")))
}

/// Pull `backends` off a form row, parsing the JSON column. Rows created
/// before the `backends` column existed have it as `NULL`; treat that as the
/// default `[open-relay]`. The boot-time backfill clears the `NULL`s
/// eventually, but the read path is tolerant in case the worker runs before
/// the backfill commits.
pub fn backends_from_model(m: &entity::form::Model) -> CoreResult<Vec<BackendBinding>> {
    match &m.backends {
        Some(v) => parse_backends(v),
        None => Ok(default_backends()),
    }
}

/// Convert a `form::Model` row into a full `FormDto`, parsing the JSON
/// columns into their typed shapes. Async because metadata lives in a separate
/// table (`form_metadata`) and is fetched per form.
pub async fn dto_from_model<C: ConnectionTrait>(
    conn: &C,
    m: entity::form::Model,
) -> CoreResult<FormDto> {
    let standard_fields = parse_standard_fields(&m.standard_fields)?;
    let custom_fields = parse_custom_fields(&m.custom_fields)?;
    let layout = layout_from_model(&m)?;
    let backends = backends_from_model(&m)?;
    let tags = tags_from_model(&m)?;
    let reps = reps_from_model(&m)?;
    let source_params = source_params_from_model(&m)?;
    let post_submission_action = post_submission_action_from_model(&m)?;
    let progress_indicator = progress_indicator_from_model(&m)?;
    let metadata = metadata_service::list(conn, m.id).await?;
    Ok(FormDto {
        id: m.id,
        owner_id: m.owner_id,
        name: m.name,
        slug: m.slug,
        standard_fields,
        custom_fields,
        layout,
        backends,
        tags,
        reps,
        source_params,
        post_submission_action,
        progress_indicator,
        metadata,
        created_at: m.created_at,
        updated_at: m.updated_at,
    })
}

pub fn public_dto_from_model(m: entity::form::Model) -> CoreResult<PublicFormDto> {
    let standard_fields = parse_standard_fields(&m.standard_fields)?;
    let custom_fields = parse_custom_fields(&m.custom_fields)?;
    let layout = layout_from_model(&m)?;
    let backends = backends_from_model(&m)?;
    let post_submission_action = post_submission_action_from_model(&m)?;
    let progress_indicator = progress_indicator_from_model(&m)?;
    Ok(PublicFormDto {
        id: m.id,
        name: m.name,
        slug: m.slug,
        standard_fields,
        custom_fields,
        layout,
        backends,
        post_submission_action,
        progress_indicator,
    })
}

pub async fn find_by_id<C: ConnectionTrait>(
    conn: &C,
    id: i32,
) -> CoreResult<Option<entity::form::Model>> {
    Ok(entity::form::Entity::find_by_id(id).one(conn).await?)
}

pub async fn find_by_slug<C: ConnectionTrait>(
    conn: &C,
    slug: &str,
) -> CoreResult<Option<entity::form::Model>> {
    Ok(entity::form::Entity::find()
        .filter(entity::form::Column::Slug.eq(slug))
        .one(conn)
        .await?)
}

pub async fn create_form<C: ConnectionTrait>(
    conn: &C,
    registry: &BackendRegistry,
    owner_id: i32,
    input: NewForm,
) -> CoreResult<entity::form::Model> {
    let name = validate_name(&input.name)?;

    let slug_input = input
        .slug
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| slugify(&name));
    let slug = validate_slug(&slug_input)?;

    if find_by_slug(conn, &slug).await?.is_some() {
        return Err(CoreError::Conflict("slug already in use".into()));
    }

    reject_conflicting_field_inputs(
        input.layout.is_some(),
        input.standard_fields.is_some(),
        !input.custom_fields.is_empty(),
    )?;

    // The layout is the source of truth; the legacy pair is its projection, so
    // both are written on every save and old embed bundles keep working.
    let layout = match input.layout {
        Some(l) => normalize_layout(l),
        None => {
            let sf = normalize_standard_fields(input.standard_fields.unwrap_or_default());
            let cf = normalize_custom_fields(input.custom_fields);
            strip_dangling_rules(normalize_layout(layout_from_legacy(&sf, &cf)))
        }
    };
    validate_layout(&layout)?;
    let (standard_fields, custom_fields) = legacy_from_layout(&layout);

    let backends = input.backends.unwrap_or_else(default_backends);
    validate_backends(conn, &backends, registry).await?;

    let tags = validate_tags(&input.tags)?;
    let reps = validate_reps(conn, &input.reps).await?;
    let source_params = validate_source_params(&input.source_params)?;
    let post_submission_action = validate_post_submission_action(&input.post_submission_action)?;

    let model = entity::form::ActiveModel {
        owner_id: ActiveValue::Set(owner_id),
        name: ActiveValue::Set(name),
        slug: ActiveValue::Set(slug),
        standard_fields: ActiveValue::Set(json_or_internal(&standard_fields)?),
        custom_fields: ActiveValue::Set(json_or_internal(&custom_fields)?),
        layout: ActiveValue::Set(Some(json_or_internal(&layout)?)),
        backends: ActiveValue::Set(Some(json_or_internal(&backends)?)),
        tags: ActiveValue::Set(if tags.is_empty() { None } else { Some(json_or_internal(&tags)?) }),
        reps: ActiveValue::Set(if reps.is_empty() { None } else { Some(json_or_internal(&reps)?) }),
        source_params: ActiveValue::Set(if source_params.is_empty() {
            None
        } else {
            Some(json_or_internal(&source_params)?)
        }),
        // The default action is stored as NULL so an untouched form stays
        // indistinguishable from one written before the column existed.
        post_submission_action: ActiveValue::Set(
            if post_submission_action == PostSubmissionAction::default() {
                None
            } else {
                Some(json_or_internal(&post_submission_action)?)
            },
        ),
        // Likewise the default indicator, so "never configured" and "configured
        // back to the default" are the same row.
        progress_indicator: ActiveValue::Set(
            if input.progress_indicator == ProgressIndicator::default() {
                None
            } else {
                Some(json_or_internal(&input.progress_indicator)?)
            },
        ),
        ..Default::default()
    };
    let inserted = model.insert(conn).await?;
    if let Some(entries) = input.metadata {
        apply_metadata(conn, inserted.id, &entries).await?;
    }
    Ok(inserted)
}

/// Upsert each provided metadata entry for the form. Used by create/update so
/// the form admin can toggle per-form options (e.g. email deduplication) in
/// the same request that edits the form. Type validation happens in
/// [`metadata_service::set`].
async fn apply_metadata<C: ConnectionTrait>(
    conn: &C,
    form_id: i32,
    entries: &[crate::metadata::MetadataEntry],
) -> CoreResult<()> {
    for entry in entries {
        metadata_service::set(conn, form_id, entry.key, entry.value.clone()).await?;
    }
    Ok(())
}

pub async fn list_forms<C: ConnectionTrait>(conn: &C, q: &ListQuery) -> CoreResult<FormList> {
    let limit = q.limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT);
    let offset = q.offset.unwrap_or(0);
    let rows = entity::form::Entity::find()
        .order_by_asc(entity::form::Column::Id)
        .limit(limit as u64)
        .offset(offset as u64)
        .all(conn)
        .await?;
    let mut items: Vec<FormDto> = Vec::with_capacity(rows.len());
    for r in rows {
        items.push(dto_from_model(conn, r).await?);
    }
    let total = entity::form::Entity::find().count(conn).await?;
    Ok(FormList {
        items,
        total,
        limit,
        offset,
    })
}

pub async fn select_list<C: ConnectionTrait>(conn: &C) -> CoreResult<Vec<FormSelectOption>> {
    let mut rows: Vec<FormSelectOption> = entity::form::Entity::find()
        .order_by_asc(entity::form::Column::Name)
        .all(conn)
        .await?
        .into_iter()
        .map(|m| FormSelectOption {
            id: m.id,
            label: m.name,
        })
        .collect();
    rows.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    Ok(rows)
}

pub async fn update_form<C: ConnectionTrait>(
    conn: &C,
    registry: &BackendRegistry,
    id: i32,
    input: UpdateForm,
) -> CoreResult<entity::form::Model> {
    let existing = find_by_id(conn, id)
        .await?
        .ok_or_else(|| CoreError::NotFound("form not found".into()))?;
    let mut active: entity::form::ActiveModel = existing.clone().into();

    if let Some(name_raw) = input.name {
        let name = validate_name(&name_raw)?;
        active.name = ActiveValue::Set(name);
    }

    if let Some(slug_raw) = input.slug {
        let slug = validate_slug(&slug_raw)?;
        if slug != existing.slug {
            if let Some(other) = find_by_slug(conn, &slug).await? {
                if other.id != id {
                    return Err(CoreError::Conflict("slug already in use".into()));
                }
            }
            active.slug = ActiveValue::Set(slug);
        }
    }

    reject_conflicting_field_inputs(
        input.layout.is_some(),
        input.standard_fields.is_some(),
        input.custom_fields.is_some(),
    )?;

    if input.layout.is_some() || input.standard_fields.is_some() || input.custom_fields.is_some() {
        let layout = match input.layout {
            Some(l) => normalize_layout(l),
            None => {
                // A legacy write. Fold it into the existing layout rather than
                // rebuilding, so a hand-ordered form isn't flattened by a
                // client that can't express order.
                let sf = input.standard_fields.map(normalize_standard_fields);
                let cf = input.custom_fields.map(normalize_custom_fields);
                strip_dangling_rules(normalize_layout(merge_legacy_into_layout(
                    &layout_from_model(&existing)?,
                    sf.as_ref(),
                    cf.as_deref(),
                )))
            }
        };
        validate_layout(&layout)?;
        let (sf, cf) = legacy_from_layout(&layout);
        active.standard_fields = ActiveValue::Set(json_or_internal(&sf)?);
        active.custom_fields = ActiveValue::Set(json_or_internal(&cf)?);
        active.layout = ActiveValue::Set(Some(json_or_internal(&layout)?));
    }

    if let Some(b) = input.backends {
        validate_backends(conn, &b, registry).await?;
        active.backends = ActiveValue::Set(Some(json_or_internal(&b)?));
    }

    if let Some(t) = input.tags {
        let tags = validate_tags(&t)?;
        active.tags = ActiveValue::Set(if tags.is_empty() { None } else { Some(json_or_internal(&tags)?) });
    }

    if let Some(r) = input.reps {
        let reps = validate_reps(conn, &r).await?;
        active.reps =
            ActiveValue::Set(if reps.is_empty() { None } else { Some(json_or_internal(&reps)?) });
    }

    if let Some(sp) = input.source_params {
        let source_params = validate_source_params(&sp)?;
        active.source_params = ActiveValue::Set(if source_params.is_empty() {
            None
        } else {
            Some(json_or_internal(&source_params)?)
        });
    }

    if let Some(a) = input.post_submission_action {
        let action = validate_post_submission_action(&a)?;
        active.post_submission_action = ActiveValue::Set(
            if action == PostSubmissionAction::default() {
                None
            } else {
                Some(json_or_internal(&action)?)
            },
        );
    }

    if let Some(p) = input.progress_indicator {
        active.progress_indicator = ActiveValue::Set(if p == ProgressIndicator::default() {
            None
        } else {
            Some(json_or_internal(&p)?)
        });
    }

    let updated = active.update(conn).await?;

    if let Some(entries) = input.metadata {
        apply_metadata(conn, updated.id, &entries).await?;
    }

    Ok(updated)
}

pub async fn delete_form<C: ConnectionTrait>(conn: &C, id: i32) -> CoreResult<()> {
    crate::submissions::service::delete_for_form(conn, id).await?;
    crate::metadata::service::delete_for_form(conn, id).await?;
    let res = entity::form::Entity::delete_by_id(id).exec(conn).await?;
    if res.rows_affected == 0 {
        return Err(CoreError::NotFound("form not found".into()));
    }
    Ok(())
}

/// Cascade hook for user deletion. Removes every form owned by `user_id`.
/// FK cleanup is in application code (no DB cascade), so this MUST be called
/// from `users::service::delete_user` inside the same transaction. Caller is
/// responsible for cleaning up submissions tied to those forms first (see
/// `submissions::service::delete_for_owner`).
pub async fn delete_for_owner<C: ConnectionTrait>(conn: &C, user_id: i32) -> CoreResult<()> {
    entity::form::Entity::delete_many()
        .filter(entity::form::Column::OwnerId.eq(user_id))
        .exec(conn)
        .await?;
    Ok(())
}

/// One-shot startup migration: any form row whose `backends` is still NULL
/// (created before the column existed) gets the default `[open-relay]`
/// binding. Idempotent — re-running is a no-op once rows are populated.
pub async fn backfill_default_backends<C: ConnectionTrait>(conn: &C) -> CoreResult<u64> {
    use sea_orm::{Statement, Value};
    let default_json = json_or_internal(&default_backends())?;
    let stmt = Statement::from_sql_and_values(
        conn.get_database_backend(),
        "UPDATE form SET backends = ? WHERE backends IS NULL",
        [Value::Json(Some(Box::new(default_json)))],
    );
    let res = conn.execute_raw(stmt).await?;
    Ok(res.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forms::ProgressStyle;

    use crate::forms::{
        FieldWidth, HeadingElement, PageBreakElement, ParagraphElement, StandardFieldConfig,
    };

    fn custom(key: &str, position: i32) -> CustomField {
        CustomField {
            key: key.into(),
            label: format!("Label {key}"),
            kind: CustomFieldType::Text,
            required: false,
            placeholder: None,
            help_text: None,
            position,
            width: FieldWidth::Full,
            default_value: None,
            visible_when: None,
        }
    }

    fn std_el(key: &str) -> FormElement {
        FormElement::Standard(StandardElement {
            key: key.into(),
            required: false,
            label: None,
            placeholder: None,
            help_text: None,
            width: FieldWidth::Full,
            default_value: None,
            input_override: None,
            visible_when: None,
        })
    }

    fn rule(conds: &[(&str, ConditionOp, Option<&str>)]) -> VisibilityRule {
        VisibilityRule {
            match_mode: crate::forms::MatchMode::All,
            conditions: conds
                .iter()
                .map(|(f, op, v)| crate::forms::Condition {
                    field: (*f).to_string(),
                    op: *op,
                    value: v.map(str::to_string),
                })
                .collect(),
        }
    }

    /// A custom field carrying a rule, for the conditional-visibility tests.
    fn custom_when(key: &str, when: VisibilityRule) -> FormElement {
        FormElement::Custom(CustomField {
            visible_when: Some(when),
            ..custom(key, 0)
        })
    }

    fn on(required: bool) -> StandardFieldConfig {
        StandardFieldConfig { enabled: true, required, label: None }
    }

    // ---- wire format ----------------------------------------------------
    //
    // The whole layout design rests on `FormElement` surviving a JSON round
    // trip: `Custom` nests a struct that itself uses `serde(flatten)` over an
    // internally-tagged enum. Adjacent tagging keeps those key-spaces apart.

    #[test]
    fn form_element_round_trips_through_json() {
        let elements = vec![
            std_el("email"),
            FormElement::Custom(custom("colour", 0)),
            FormElement::Heading(HeadingElement {
                text: "About you".into(),
                level: 3,
                visible_when: None,
            }),
            FormElement::Paragraph(ParagraphElement {
                text: "Tell us more.".into(),
                visible_when: None,
            }),
            FormElement::Divider,
            FormElement::PageBreak(PageBreakElement { title: Some("Step 2".into()) }),
        ];
        let json = serde_json::to_string(&elements).unwrap();
        let back: Vec<FormElement> = serde_json::from_str(&json).unwrap();
        assert_eq!(elements, back);
    }

    #[test]
    fn custom_element_config_is_byte_identical_to_custom_field_json() {
        // What makes the derivation/projection pure moves.
        let f = custom("colour", 0);
        let el = FormElement::Custom(f.clone());
        let v = serde_json::to_value(&el).unwrap();
        assert_eq!(v["element"], "custom");
        assert_eq!(v["config"], serde_json::to_value(&f).unwrap());
    }

    #[test]
    fn select_options_survive_layout_round_trip() {
        let mut f = custom("colour", 0);
        f.kind = CustomFieldType::Select { options: vec!["Red".into(), "Blue".into()] };
        let el = FormElement::Custom(f);
        let back: FormElement =
            serde_json::from_str(&serde_json::to_string(&el).unwrap()).unwrap();
        assert_eq!(el, back);
    }

    #[test]
    fn divider_round_trips_without_a_config_key() {
        let json = serde_json::to_string(&FormElement::Divider).unwrap();
        assert_eq!(json, r#"{"element":"divider"}"#);
        let back: FormElement = serde_json::from_str(&json).unwrap();
        assert_eq!(back, FormElement::Divider);
    }

    #[test]
    fn heading_level_round_trips_as_a_number() {
        let el = FormElement::Heading(HeadingElement {
            text: "Hi".into(),
            level: 4,
            visible_when: None,
        });
        let v = serde_json::to_value(&el).unwrap();
        assert_eq!(v["config"]["level"], serde_json::json!(4));
    }

    #[test]
    fn unknown_element_tag_is_rejected() {
        assert!(serde_json::from_str::<FormElement>(r#"{"element":"headng"}"#).is_err());
    }

    #[test]
    fn typo_in_element_body_is_rejected_not_silently_dropped() {
        // The reason for adjacent tagging: `deny_unknown_fields` is possible
        // here, so a misspelled key 400s instead of saving as empty.
        assert!(
            serde_json::from_str::<FormElement>(
                r#"{"element":"heading","config":{"txet":"Welcome","level":2}}"#
            )
            .is_err()
        );
    }

    // ---- derivation / projection ---------------------------------------

    #[test]
    fn layout_from_legacy_matches_the_pre_layout_render_order() {
        let mut sf = StandardFieldsConfig::all_disabled();
        sf.email = on(true);
        sf.first_name = on(false);
        sf.message = on(false);
        let cf = vec![custom("b", 1), custom("a", 0)];

        let derived = layout_from_legacy(&sf, &cf);
        let keys: Vec<&str> = derived.iter().filter_map(|e| e.field_key()).collect();
        // Standards in catalogue order (first_name before email, message last),
        // then customs by position — exactly what Form.tsx drew.
        assert_eq!(keys, vec!["first_name", "email", "message", "a", "b"]);
    }

    #[test]
    fn legacy_from_layout_projects_enabled_required_and_positions() {
        let layout = vec![
            FormElement::Custom(custom("a", 7)),
            FormElement::Divider,
            FormElement::Standard(StandardElement {
                required: true,
                label: Some("Work email".into()),
                ..StandardElement::from_legacy("email", &on(true))
            }),
            FormElement::Custom(custom("b", 3)),
        ];
        let (sf, cf) = legacy_from_layout(&layout);

        assert!(sf.email.enabled && sf.email.required);
        assert_eq!(sf.email.label.as_deref(), Some("Work email"));
        assert!(!sf.first_name.enabled, "keys absent from the layout are disabled");
        // Positions are renumbered from layout order, and the divider is dropped.
        assert_eq!(cf.iter().map(|c| (c.key.as_str(), c.position)).collect::<Vec<_>>(),
                   vec![("a", 0), ("b", 1)]);
    }

    #[test]
    fn legacy_layout_legacy_round_trip_is_identity() {
        let mut sf = StandardFieldsConfig::all_disabled();
        sf.email = StandardFieldConfig { enabled: true, required: true, label: Some("E".into()) };
        sf.phone = on(false);
        let cf = vec![custom("a", 0), custom("b", 1)];

        let (sf2, cf2) = legacy_from_layout(&layout_from_legacy(&sf, &cf));
        assert_eq!(
            serde_json::to_value(&sf).unwrap(),
            serde_json::to_value(&sf2).unwrap()
        );
        assert_eq!(cf, cf2);
    }

    #[test]
    fn layout_legacy_layout_drops_decoration_and_collapses_interleaving() {
        // Documented downlevel loss: only affects what a pre-layout embed
        // bundle draws, never what the server validates.
        let layout = vec![
            FormElement::Custom(custom("a", 0)),
            FormElement::Heading(HeadingElement {
                text: "H".into(),
                level: 2,
                visible_when: None,
            }),
            std_el("email"),
        ];
        let (sf, cf) = legacy_from_layout(&layout);
        let round = layout_from_legacy(&sf, &cf);
        assert_eq!(
            round.iter().filter_map(|e| e.field_key()).collect::<Vec<_>>(),
            vec!["email", "a"],
            "standards come before customs once projected"
        );
        assert!(!round.iter().any(|e| matches!(e, FormElement::Heading(_))));
    }

    #[test]
    fn projected_columns_keep_a_disabled_field_zeroed() {
        // Guards the pre-work normalisation: a disabled key can carry no
        // required/label, so the round trip above stays an identity.
        let (sf, _) = legacy_from_layout(&[std_el("email")]);
        assert!(!sf.phone.enabled && !sf.phone.required && sf.phone.label.is_none());
    }

    #[test]
    fn normalize_standard_fields_zeroes_disabled_entries() {
        let mut sf = StandardFieldsConfig::all_disabled();
        sf.phone = StandardFieldConfig {
            enabled: false,
            required: true,
            label: Some("Mobile".into()),
        };
        let sf = normalize_standard_fields(sf);
        assert!(!sf.phone.required && sf.phone.label.is_none());
    }

    // ---- legacy merge ---------------------------------------------------

    #[test]
    fn merge_legacy_updates_a_standard_in_place_without_moving_it() {
        let existing = vec![
            FormElement::Custom(custom("a", 0)),
            std_el("email"),
        ];
        let mut sf = StandardFieldsConfig::all_disabled();
        sf.email = on(true);
        let out = merge_legacy_into_layout(&existing, Some(&sf), None);

        assert_eq!(out.iter().filter_map(|e| e.field_key()).collect::<Vec<_>>(), vec!["a", "email"],
                   "the custom field stays ahead of the standard one");
        match &out[1] {
            FormElement::Standard(s) => assert!(s.required),
            other => panic!("expected standard, got {other:?}"),
        }
    }

    #[test]
    fn merge_legacy_preserves_layout_only_extras_on_update() {
        let existing = vec![FormElement::Standard(StandardElement {
            placeholder: Some("you@example.com".into()),
            width: FieldWidth::Half,
            ..StandardElement::from_legacy("email", &on(false))
        })];
        let mut sf = StandardFieldsConfig::all_disabled();
        sf.email = on(true);
        let out = merge_legacy_into_layout(&existing, Some(&sf), None);
        match &out[0] {
            FormElement::Standard(s) => {
                assert_eq!(s.placeholder.as_deref(), Some("you@example.com"));
                assert_eq!(s.width, FieldWidth::Half);
                assert!(s.required, "the legacy payload still wins on `required`");
            }
            other => panic!("expected standard, got {other:?}"),
        }
    }

    #[test]
    fn merge_legacy_removes_a_disabled_standard() {
        let existing = vec![std_el("email"), std_el("phone")];
        let mut sf = StandardFieldsConfig::all_disabled();
        sf.email = on(false);
        let out = merge_legacy_into_layout(&existing, Some(&sf), None);
        assert_eq!(out.iter().filter_map(|e| e.field_key()).collect::<Vec<_>>(), vec!["email"]);
    }

    #[test]
    fn merge_legacy_inserts_a_newly_enabled_standard_at_catalogue_position() {
        // A legacy client has no ordering vocabulary, so `phone` must land
        // between first_name and email's catalogue neighbours, not at the end.
        let existing = vec![std_el("first_name"), std_el("message")];
        let mut sf = StandardFieldsConfig::all_disabled();
        sf.first_name = on(false);
        sf.message = on(false);
        sf.phone = on(false);
        let out = merge_legacy_into_layout(&existing, Some(&sf), None);
        assert_eq!(
            out.iter().filter_map(|e| e.field_key()).collect::<Vec<_>>(),
            vec!["first_name", "phone", "message"]
        );
    }

    #[test]
    fn merge_legacy_does_not_push_a_new_standard_past_a_page_break() {
        let existing = vec![
            std_el("country"),
            FormElement::PageBreak(PageBreakElement::default()),
            FormElement::Custom(custom("a", 0)),
        ];
        let mut sf = StandardFieldsConfig::all_disabled();
        sf.country = on(false);
        sf.email = on(false); // catalogue-earlier than country -> goes before it
        let out = merge_legacy_into_layout(&existing, Some(&sf), None);
        let break_at = out.iter().position(|e| matches!(e, FormElement::PageBreak(_))).unwrap();
        let email_at = out
            .iter()
            .position(|e| e.field_key() == Some("email"))
            .unwrap();
        assert!(email_at < break_at, "new standard stays on the first page");
    }

    #[test]
    fn merge_legacy_of_a_derived_layout_equals_plain_derivation() {
        // Invariant M1: for a form the builder never touched, a legacy write
        // behaves exactly as it did before layouts existed.
        let mut old = StandardFieldsConfig::all_disabled();
        old.email = on(true);
        let old_cf = vec![custom("a", 0)];

        let mut new = StandardFieldsConfig::all_disabled();
        new.email = on(true);
        new.phone = on(false);
        let new_cf = vec![custom("a", 0), custom("b", 1)];

        let merged = normalize_layout(merge_legacy_into_layout(
            &layout_from_legacy(&old, &old_cf),
            Some(&new),
            Some(&new_cf),
        ));
        assert_eq!(merged, normalize_layout(layout_from_legacy(&new, &new_cf)));
    }

    #[test]
    fn merge_legacy_keeps_custom_slots_and_appends_new_ones() {
        let existing = vec![
            FormElement::Custom(custom("a", 0)),
            std_el("email"),
            FormElement::Custom(custom("b", 1)),
        ];
        let incoming = vec![custom("a", 0), custom("b", 1), custom("c", 2)];
        let out = merge_legacy_into_layout(&existing, None, Some(&incoming));
        assert_eq!(
            out.iter().filter_map(|e| e.field_key()).collect::<Vec<_>>(),
            vec!["a", "email", "b", "c"],
            "existing customs keep their interleaved slots; new ones append"
        );
    }

    #[test]
    fn merge_legacy_removes_a_missing_custom_and_clears_on_empty_vec() {
        let existing = vec![
            FormElement::Custom(custom("a", 0)),
            FormElement::Heading(HeadingElement {
                text: "H".into(),
                level: 2,
                visible_when: None,
            }),
            FormElement::Custom(custom("b", 1)),
        ];
        let out = merge_legacy_into_layout(&existing, None, Some(&[custom("a", 0)]));
        assert_eq!(out.iter().filter_map(|e| e.field_key()).collect::<Vec<_>>(), vec!["a"]);

        let cleared = merge_legacy_into_layout(&existing, None, Some(&[]));
        assert!(cleared.iter().all(|e| e.field_key().is_none()));
        assert_eq!(cleared.len(), 1, "the heading survives a custom-field clear");
    }

    #[test]
    fn merge_legacy_with_both_none_is_a_noop() {
        let existing = vec![std_el("email"), FormElement::Divider];
        assert_eq!(merge_legacy_into_layout(&existing, None, None), existing);
    }

    #[test]
    fn merge_legacy_honours_a_reorder_without_moving_decoration() {
        let existing = vec![
            FormElement::Custom(custom("a", 0)),
            FormElement::Divider,
            FormElement::Custom(custom("b", 1)),
        ];
        let out = merge_legacy_into_layout(
            &existing,
            None,
            Some(&[custom("b", 0), custom("a", 1)]),
        );
        assert_eq!(out.iter().filter_map(|e| e.field_key()).collect::<Vec<_>>(), vec!["b", "a"]);
        assert!(matches!(out[1], FormElement::Divider), "the divider holds its slot");
    }

    // ---- validation -----------------------------------------------------

    #[test]
    fn validate_layout_rejects_a_duplicate_standard_key() {
        assert!(validate_layout(&[std_el("email"), std_el("email")]).is_err());
    }

    #[test]
    fn validate_layout_rejects_an_unknown_standard_key() {
        assert!(validate_layout(&[std_el("nope")]).is_err());
    }

    #[test]
    fn validate_layout_delegates_custom_field_rules() {
        // `email` collides with a standard key — caught by validate_custom_fields.
        assert!(validate_layout(&[FormElement::Custom(custom("email", 0))]).is_err());

        let mut f = custom("colour", 0);
        f.kind = CustomFieldType::Select { options: vec![] };
        assert!(validate_layout(&[FormElement::Custom(f)]).is_err());
    }

    #[test]
    fn validate_layout_rejects_degenerate_page_breaks() {
        let brk = || FormElement::PageBreak(PageBreakElement::default());
        assert!(validate_layout(&[brk(), std_el("email")]).is_err(), "leading");
        assert!(validate_layout(&[std_el("email"), brk()]).is_err(), "trailing");
        assert!(
            validate_layout(&[std_el("email"), brk(), brk(), std_el("phone")]).is_err(),
            "consecutive"
        );
        assert!(validate_layout(&[std_el("email"), brk(), std_el("phone")]).is_ok());
    }

    #[test]
    fn validate_layout_rejects_select_override_on_a_non_country_field() {
        let el = FormElement::Standard(StandardElement {
            input_override: Some(StandardInputVariant::Select),
            ..StandardElement::from_legacy("email", &on(false))
        });
        assert!(validate_layout(&[el]).is_err());

        let ok = FormElement::Standard(StandardElement {
            input_override: Some(StandardInputVariant::Select),
            ..StandardElement::from_legacy("country", &on(false))
        });
        assert!(validate_layout(&[ok]).is_ok());
    }

    #[test]
    fn reject_conflicting_field_inputs_rejects_layout_plus_legacy() {
        assert!(reject_conflicting_field_inputs(true, true, false).is_err());
        assert!(reject_conflicting_field_inputs(true, false, true).is_err());
        assert!(reject_conflicting_field_inputs(true, false, false).is_ok());
        assert!(reject_conflicting_field_inputs(false, true, true).is_ok());
    }

    #[test]
    fn normalize_layout_clamps_heading_level_and_trims_copy() {
        let out = normalize_layout(vec![
            FormElement::Heading(HeadingElement {
                text: "  Hi  ".into(),
                level: 9,
                visible_when: None,
            }),
            FormElement::Custom(custom("a", 42)),
        ]);
        match &out[0] {
            FormElement::Heading(h) => {
                assert_eq!(h.text, "Hi");
                assert_eq!(h.level, 6);
            }
            other => panic!("expected heading, got {other:?}"),
        }
        match &out[1] {
            FormElement::Custom(c) => assert_eq!(c.position, 0, "renumbered from layout order"),
            other => panic!("expected custom, got {other:?}"),
        }
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("  Contact   Us!! "), "contact-us");
        assert_eq!(slugify("Über—Form"), "ber-form");
        assert_eq!(slugify("---"), "");
    }

    #[test]
    fn slug_validation_accepts_well_formed() {
        assert!(validate_slug("contact-us").is_ok());
        assert!(validate_slug("form1").is_ok());
    }

    #[test]
    fn slug_validation_rejects_bad() {
        assert!(validate_slug("").is_err());
        assert!(validate_slug("-bad").is_err());
        assert!(validate_slug("bad-").is_err());
        assert!(validate_slug("two--dash").is_err());
        assert!(validate_slug("UpperCase").is_err());
        assert!(validate_slug("has space").is_err());
    }

    #[test]
    fn custom_field_keys_must_be_unique() {
        let fields = vec![
            CustomField {
                key: "shoe_size".into(),
                label: "Shoe size".into(),
                kind: CustomFieldType::Text,
                required: false,
                placeholder: None,
                help_text: None,
                position: 0,
                width: Default::default(),
                default_value: None,
                visible_when: None,
            },
            CustomField {
                key: "shoe_size".into(),
                label: "Again".into(),
                kind: CustomFieldType::Text,
                required: false,
                placeholder: None,
                help_text: None,
                position: 1,
                width: Default::default(),
                default_value: None,
                visible_when: None,
            },
        ];
        assert!(validate_custom_fields(&fields).is_err());
    }

    #[test]
    fn custom_field_select_requires_options() {
        let fields = vec![CustomField {
            key: "color".into(),
            label: "Color".into(),
            kind: CustomFieldType::Select { options: vec![] },
            required: false,
            placeholder: None,
            help_text: None,
            position: 0,
            width: Default::default(),
            default_value: None,
            visible_when: None,
        }];
        assert!(validate_custom_fields(&fields).is_err());
    }

    #[test]
    fn custom_field_key_accepts_backend_specific_formats() {
        // GoHighLevel unique keys and raw field ids must round-trip unchanged.
        for key in ["contact.how_did_you_hear", "aBc123XyZ", "lead-source"] {
            let fields = vec![CustomField {
                key: key.into(),
                label: "Mapped field".into(),
                kind: CustomFieldType::Text,
                required: false,
                placeholder: None,
                help_text: None,
                position: 0,
                width: Default::default(),
                default_value: None,
                visible_when: None,
            }];
            assert!(
                validate_custom_fields(&fields).is_ok(),
                "expected key '{key}' to be accepted"
            );
        }
    }

    #[test]
    fn custom_field_key_rejects_whitespace() {
        let fields = vec![CustomField {
            key: "has space".into(),
            label: "Bad".into(),
            kind: CustomFieldType::Text,
            required: false,
            placeholder: None,
            help_text: None,
            position: 0,
            width: Default::default(),
            default_value: None,
            visible_when: None,
        }];
        assert!(validate_custom_fields(&fields).is_err());
    }

    #[test]
    fn source_params_reject_reserved_rep_and_dedup() {
        let sp = |p: &str| SourceParam {
            param: p.into(),
            tag_prefix: None,
        };
        // Reserved name is rejected.
        assert!(validate_source_params(&[sp("rep")]).is_err());
        // Duplicates are rejected.
        assert!(validate_source_params(&[sp("event"), sp(" event ")]).is_err());
        // Trims, keeps prefix.
        let out = validate_source_params(&[SourceParam {
            param: " event ".into(),
            tag_prefix: Some("  evt  ".into()),
        }])
        .unwrap();
        assert_eq!(out[0].param, "event");
        assert_eq!(out[0].tag_prefix.as_deref(), Some("evt"));
    }

    #[test]
    fn custom_field_key_cannot_collide_with_standard() {
        let fields = vec![CustomField {
            key: "email".into(),
            label: "Custom email".into(),
            kind: CustomFieldType::Text,
            required: false,
            placeholder: None,
            help_text: None,
            position: 0,
            width: Default::default(),
            default_value: None,
            visible_when: None,
        }];
        assert!(validate_custom_fields(&fields).is_err());
    }

    // ---- progress indicator ----------------------------------------------

    #[test]
    fn default_progress_indicator_is_a_bar_with_its_percentage() {
        let d = ProgressIndicator::default();
        assert_eq!(d.style, ProgressStyle::Bar);
        assert!(d.show_percent);
    }

    #[test]
    fn progress_indicator_round_trips_through_json() {
        let p = ProgressIndicator {
            style: ProgressStyle::Steps,
            show_percent: false,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, r#"{"style":"steps","show_percent":false}"#);
        assert_eq!(serde_json::from_str::<ProgressIndicator>(&json).unwrap(), p);
    }

    #[test]
    fn partial_progress_indicator_fills_in_the_defaults() {
        // `#[serde(default)]` on the struct, not just the fields, so a client
        // that only sets the style doesn't have to know about show_percent.
        let p: ProgressIndicator = serde_json::from_str(r#"{"style":"none"}"#).unwrap();
        assert_eq!(p.style, ProgressStyle::None);
        assert!(p.show_percent);
        assert_eq!(
            serde_json::from_str::<ProgressIndicator>("{}").unwrap(),
            ProgressIndicator::default()
        );
    }

    #[test]
    fn unknown_progress_indicator_keys_and_styles_are_rejected() {
        assert!(serde_json::from_str::<ProgressIndicator>(r#"{"styl":"bar"}"#).is_err());
        assert!(serde_json::from_str::<ProgressIndicator>(r#"{"style":"pie"}"#).is_err());
    }

    // ---- post-submission action ------------------------------------------

    #[test]
    fn post_submission_action_round_trips_through_json() {
        let action = PostSubmissionAction::Message(MessageAction {
            message: Some("Thanks!".into()),
            allow_resubmit: true,
            resubmit_label: Some("Add another".into()),
        });
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(
            json,
            r#"{"action":"message","config":{"message":"Thanks!","allow_resubmit":true,"resubmit_label":"Add another"}}"#
        );
        let back: PostSubmissionAction = serde_json::from_str(&json).unwrap();
        assert_eq!(back, action);
    }

    #[test]
    fn default_post_submission_action_is_an_empty_message() {
        assert_eq!(
            PostSubmissionAction::default(),
            PostSubmissionAction::Message(MessageAction::default())
        );
        // Optional fields are skipped, so the default is the smallest possible
        // body — this is the value create/update compare against to store NULL.
        let json = serde_json::to_string(&PostSubmissionAction::default()).unwrap();
        assert_eq!(json, r#"{"action":"message","config":{"allow_resubmit":false}}"#);
    }

    #[test]
    fn unknown_post_submission_action_tag_is_rejected() {
        let err =
            serde_json::from_str::<PostSubmissionAction>(r#"{"action":"close_tab","config":{}}"#);
        assert!(err.is_err());
    }

    #[test]
    fn typo_in_post_submission_config_is_rejected_not_silently_dropped() {
        // Without deny_unknown_fields this would persist as a message action
        // with no message, i.e. the admin's copy would vanish undetectably.
        let err = serde_json::from_str::<PostSubmissionAction>(
            r#"{"action":"message","config":{"mesage":"Thanks!"}}"#,
        );
        assert!(err.is_err());
    }

    #[test]
    fn message_and_label_are_trimmed_and_blanks_become_none() {
        let out = validate_post_submission_action(&PostSubmissionAction::Message(MessageAction {
            message: Some("  Thanks!  ".into()),
            allow_resubmit: true,
            resubmit_label: Some("   ".into()),
        }))
        .unwrap();
        let PostSubmissionAction::Message(m) = out else {
            panic!("expected a message action")
        };
        assert_eq!(m.message.as_deref(), Some("Thanks!"));
        assert!(m.allow_resubmit);
        assert_eq!(m.resubmit_label, None);
    }

    #[test]
    fn oversize_message_is_rejected() {
        let long = "x".repeat(MAX_POST_SUBMISSION_MESSAGE_LEN + 1);
        assert!(
            validate_post_submission_action(&PostSubmissionAction::Message(MessageAction {
                message: Some(long),
                allow_resubmit: false,
                resubmit_label: None,
            }))
            .is_err()
        );
    }

    #[test]
    fn redirect_url_accepts_absolute_http_and_https() {
        for url in [
            "https://example.com/thanks",
            "http://example.com",
            "HTTPS://Example.com/Thanks?a=1#b",
        ] {
            let out = validate_redirect_url(url).unwrap_or_else(|e| panic!("{url}: {e:?}"));
            // Case is preserved — only the scheme check lowercases.
            assert_eq!(out, url);
        }
        assert_eq!(validate_redirect_url("  https://example.com  ").unwrap(), "https://example.com");
    }

    #[test]
    fn redirect_url_rejects_anything_that_is_not_absolute_http() {
        // Every one of these would be script execution or an unintended
        // navigation on a third-party host page.
        for url in [
            "",
            "   ",
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "java\nscript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "vbscript:msgbox(1)",
            "//evil.example/thanks",
            "/thanks",
            "thanks",
            "https:///thanks",
            "https://",
            "https://exa mple.com",
        ] {
            assert!(
                validate_redirect_url(url).is_err(),
                "expected {url:?} to be rejected"
            );
        }
        assert!(validate_redirect_url(&format!("https://e.com/{}", "x".repeat(MAX_REDIRECT_URL_LEN))).is_err());
    }

    #[test]
    fn redirect_action_is_validated_through_the_public_entry_point() {
        assert!(
            validate_post_submission_action(&PostSubmissionAction::Redirect(RedirectAction {
                url: "javascript:alert(1)".into(),
            }))
            .is_err()
        );
        let out = validate_post_submission_action(&PostSubmissionAction::Redirect(RedirectAction {
            url: " https://example.com/thanks ".into(),
        }))
        .unwrap();
        assert_eq!(
            out,
            PostSubmissionAction::Redirect(RedirectAction {
                url: "https://example.com/thanks".into(),
            })
        );
    }

    // ---- conditional visibility -----------------------------------------

    #[test]
    fn a_rule_round_trips_through_json() {
        let el = custom_when("b", rule(&[("a", ConditionOp::Equals, Some("yes"))]));
        let json = serde_json::to_value(&el).unwrap();
        assert_eq!(
            json["config"]["visible_when"],
            serde_json::json!({ "conditions": [{ "field": "a", "op": "equals", "value": "yes" }] }),
            "`match` stays off the wire while it is the default"
        );
        let back: FormElement = serde_json::from_value(json).unwrap();
        assert_eq!(back, el);
    }

    #[test]
    fn a_typo_inside_a_rule_is_rejected_not_dropped() {
        let json = serde_json::json!({
            "element": "standard",
            "config": {
                "key": "city",
                "visible_when": { "conditions": [{ "field": "a", "op": "equals", "valeu": "x" }] }
            }
        });
        assert!(serde_json::from_value::<FormElement>(json).is_err());
    }

    #[test]
    fn validate_layout_requires_the_controller_to_appear_earlier() {
        // Forward reference: `b` names `a`, which comes after it.
        let forward = vec![
            custom_when("b", rule(&[("a", ConditionOp::Equals, Some("x"))])),
            FormElement::Custom(custom("a", 1)),
        ];
        assert!(validate_layout(&forward).is_err());

        // The same two elements the other way round are fine.
        let ok = vec![
            FormElement::Custom(custom("a", 0)),
            custom_when("b", rule(&[("a", ConditionOp::Equals, Some("x"))])),
        ];
        assert!(validate_layout(&ok).is_ok());
    }

    #[test]
    fn validate_layout_rejects_a_self_reference_and_an_unknown_key() {
        let me = vec![custom_when("a", rule(&[("a", ConditionOp::IsNotEmpty, None)]))];
        assert!(validate_layout(&me).is_err(), "a field cannot depend on itself");

        let ghost = vec![custom_when("a", rule(&[("nope", ConditionOp::IsNotEmpty, None)]))];
        assert!(validate_layout(&ghost).is_err());
    }

    #[test]
    fn validate_layout_checks_operand_arity() {
        let missing = vec![
            FormElement::Custom(custom("a", 0)),
            custom_when("b", rule(&[("a", ConditionOp::Equals, None)])),
        ];
        assert!(validate_layout(&missing).is_err(), "equals needs a value");

        let extra = vec![
            FormElement::Custom(custom("a", 0)),
            custom_when("b", rule(&[("a", ConditionOp::IsEmpty, Some("x"))])),
        ];
        assert!(validate_layout(&extra).is_err(), "is_empty takes no value");
    }

    #[test]
    fn validate_layout_allows_is_checked_only_against_a_checkbox() {
        let text = vec![
            FormElement::Custom(custom("a", 0)),
            custom_when("b", rule(&[("a", ConditionOp::IsChecked, None)])),
        ];
        assert!(validate_layout(&text).is_err());

        let checkbox = vec![
            FormElement::Custom(CustomField {
                kind: CustomFieldType::Checkbox,
                ..custom("a", 0)
            }),
            custom_when("b", rule(&[("a", ConditionOp::IsChecked, None)])),
        ];
        assert!(validate_layout(&checkbox).is_ok());
    }

    #[test]
    fn validate_layout_caps_condition_count() {
        let many: Vec<(&str, ConditionOp, Option<&str>)> =
            (0..MAX_CONDITIONS + 1).map(|_| ("a", ConditionOp::IsNotEmpty, None)).collect();
        let layout = vec![
            FormElement::Custom(custom("a", 0)),
            custom_when("b", rule(&many)),
        ];
        assert!(validate_layout(&layout).is_err());
    }

    #[test]
    fn a_decoration_element_can_carry_a_rule() {
        let layout = vec![
            FormElement::Custom(custom("a", 0)),
            FormElement::Heading(HeadingElement {
                text: "Billing".into(),
                level: 2,
                visible_when: Some(rule(&[("a", ConditionOp::Equals, Some("no"))])),
            }),
        ];
        assert!(validate_layout(&layout).is_ok());
    }

    #[test]
    fn normalize_layout_trims_a_rule_and_drops_an_empty_one() {
        let out = normalize_layout(vec![
            FormElement::Custom(custom("a", 0)),
            custom_when("b", rule(&[(" a ", ConditionOp::Equals, Some("  yes  "))])),
            custom_when("c", VisibilityRule { match_mode: crate::forms::MatchMode::All, conditions: vec![] }),
        ]);
        let cond = &out[1].visible_when().unwrap().conditions[0];
        assert_eq!(cond.field, "a");
        assert_eq!(cond.value.as_deref(), Some("yes"));
        assert!(out[2].visible_when().is_none(), "a rule with no conditions is not a rule");
    }

    #[test]
    fn a_legacy_write_repairs_a_rule_it_stranded_instead_of_rejecting_it() {
        // A legacy client disables the controller. It has no vocabulary for
        // conditions and cannot see the rule, so 400ing it would be unactionable
        // — the dependent just becomes unconditional.
        let existing = vec![
            std_el("phone"),
            FormElement::Custom(CustomField {
                visible_when: Some(rule(&[("phone", ConditionOp::IsNotEmpty, None)])),
                ..custom("note", 0)
            }),
        ];
        let sf = StandardFieldsConfig::all_disabled();
        let merged = strip_dangling_rules(normalize_layout(merge_legacy_into_layout(
            &existing,
            Some(&sf),
            None,
        )));
        assert_eq!(merged.iter().filter_map(|e| e.field_key()).collect::<Vec<_>>(), vec!["note"]);
        assert!(merged[0].visible_when().is_none());
        assert!(validate_layout(&merged).is_ok());
    }

    #[test]
    fn a_legacy_write_repairs_a_controller_that_moved_behind_its_dependent() {
        // Pass 2 inserts a newly-enabled standard at its catalogue position,
        // which here lands *after* the custom that depends on it.
        let existing = vec![FormElement::Custom(CustomField {
            visible_when: Some(rule(&[("phone", ConditionOp::IsNotEmpty, None)])),
            ..custom("note", 0)
        })];
        let mut sf = StandardFieldsConfig::all_disabled();
        sf.phone = on(false);
        let merged = strip_dangling_rules(normalize_layout(merge_legacy_into_layout(
            &existing,
            Some(&sf),
            None,
        )));
        assert!(validate_layout(&merged).is_ok(), "a legacy PATCH must never 400 on a rule it can't see");
        assert!(merged.iter().all(|e| e.visible_when().is_none()));
    }

    #[test]
    fn merge_legacy_keeps_a_custom_fields_rule_a_client_cannot_express() {
        // Mirrors `merge_legacy_preserves_layout_only_extras_on_update` for the
        // custom branch, which replaces the field wholesale.
        let when = rule(&[("a", ConditionOp::Equals, Some("yes"))]);
        let existing = vec![
            FormElement::Custom(custom("a", 0)),
            FormElement::Custom(CustomField {
                visible_when: Some(when.clone()),
                ..custom("b", 1)
            }),
        ];
        // An old client echoes the fields back without the key it never knew.
        let incoming = vec![custom("a", 0), custom("b", 1)];
        let out = merge_legacy_into_layout(&existing, None, Some(&incoming));
        assert_eq!(out[1].visible_when(), Some(&when));
    }

    #[test]
    fn legacy_from_layout_drops_a_standard_rule_but_carries_a_custom_one() {
        let when = rule(&[("a", ConditionOp::Equals, Some("yes"))]);
        let layout = vec![
            FormElement::Custom(custom("a", 0)),
            FormElement::Standard(StandardElement {
                visible_when: Some(when.clone()),
                ..StandardElement::from_legacy("city", &on(false))
            }),
            FormElement::Custom(CustomField {
                visible_when: Some(when.clone()),
                ..custom("b", 1)
            }),
        ];
        let (sf, cf) = legacy_from_layout(&layout);
        // `StandardFieldConfig` has nowhere to put a rule; `CustomField` is
        // stored verbatim, so its rule rides along.
        assert!(sf.city.enabled);
        assert_eq!(cf[1].visible_when, Some(when));
    }

}
