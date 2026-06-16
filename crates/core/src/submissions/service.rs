//! Submission persistence + validation.
//!
//! `create_submission` is the heart of the public POST: it validates the
//! incoming payload against the form schema, inserts the submission row,
//! then fans out one `submission_delivery` row per backend bound to the
//! form. Everything inside a single transaction so a partial fan-out can't
//! leave a submission in "stored but undeliverable" limbo.

use std::collections::{HashMap, HashSet};

use anyhow::anyhow;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::{Map as JsonMap, Value as JsonValue};

use super::{
    ListQuery, NewSubmissionPayload, RetryDeliveriesRequest, RetryDeliveriesResponse,
    SubmissionDeliveryDto, SubmissionDto, SubmissionList,
};
use crate::error::{CoreError, CoreResult};
use crate::forms::{
    BackendBinding, CustomField, CustomFieldType, STANDARD_FIELD_KEYS, StandardFieldsConfig,
    service as forms_service,
};
use crate::metadata::{MetadataKey, service as metadata_service};
use crate::reps::service as reps_service;

const DEFAULT_LIST_LIMIT: u32 = 50;
const MAX_LIST_LIMIT: u32 = 200;
/// Hidden anti-bot field the renderer includes off-screen. A real user never
/// fills it; a non-empty value means a bot, so we reject. Kept generic in the
/// error so a bot can't learn the field name from the response.
const HONEYPOT_KEY: &str = "_hp";
/// Reserved payload key carrying the QR landing page's source context (the
/// renderer forwards the page's URL query params here): `{ "rep": "jane",
/// "event": "mjbiz-2026" }`. Pulled out before field validation.
const SOURCE_KEY: &str = "_source";
const MAX_STANDARD_FIELD_LEN: usize = 1000;
const MAX_MESSAGE_LEN: usize = 10_000;
/// Cap on a single captured source-param value (becomes a tag downstream).
const MAX_SOURCE_VALUE_LEN: usize = 255;

pub const STATUS_PENDING: &str = "pending";
pub const STATUS_IN_PROGRESS: &str = "in_progress";
pub const STATUS_SUCCEEDED: &str = "succeeded";
pub const STATUS_PERMANENT_FAILURE: &str = "permanent_failure";
pub const STATUS_EXHAUSTED: &str = "exhausted";

/// Extracted standard-field values, keyed by standard field key. Only keys
/// for fields that are enabled on the form and present in the payload appear.
type StandardValues = HashMap<&'static str, String>;

/// Validate the payload against the form, returning the extracted standard
/// values + the remaining (custom) data. Returns `BadRequest` on missing
/// required fields, oversized strings, or type mismatches.
fn validate_and_split(
    payload: NewSubmissionPayload,
    standard_cfg: &StandardFieldsConfig,
    custom_fields: &[CustomField],
) -> CoreResult<(StandardValues, JsonValue)> {
    let mut input = payload.0;
    let mut standard: StandardValues = HashMap::new();

    let cfg_for = |key: &str| match key {
        "first_name" => &standard_cfg.first_name,
        "last_name" => &standard_cfg.last_name,
        "email" => &standard_cfg.email,
        "phone" => &standard_cfg.phone,
        "company" => &standard_cfg.company,
        "job_title" => &standard_cfg.job_title,
        "website" => &standard_cfg.website,
        "message" => &standard_cfg.message,
        "address_line_1" => &standard_cfg.address_line_1,
        "address_line_2" => &standard_cfg.address_line_2,
        "city" => &standard_cfg.city,
        "state" => &standard_cfg.state,
        "postal_code" => &standard_cfg.postal_code,
        "country" => &standard_cfg.country,
        _ => unreachable!("validate_and_split called with non-standard key"),
    };

    for &key in STANDARD_FIELD_KEYS {
        let cfg = cfg_for(key);
        let raw = input.remove(key);
        if !cfg.enabled {
            continue;
        }
        let trimmed = match raw {
            Some(JsonValue::String(s)) => {
                let t = s.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            }
            Some(JsonValue::Null) | None => None,
            Some(other) => {
                return Err(CoreError::BadRequest(format!(
                    "standard field '{key}' must be a string, got {}",
                    json_kind(&other)
                )));
            }
        };
        let max = if key == "message" {
            MAX_MESSAGE_LEN
        } else {
            MAX_STANDARD_FIELD_LEN
        };
        if let Some(v) = &trimmed {
            if v.chars().count() > max {
                return Err(CoreError::BadRequest(format!(
                    "standard field '{key}' exceeds {max} characters"
                )));
            }
        }
        match (cfg.required, trimmed) {
            (true, None) => {
                return Err(CoreError::BadRequest(format!(
                    "required field '{key}' missing"
                )));
            }
            (_, Some(v)) => {
                standard.insert(key, v);
            }
            (false, None) => {}
        }
    }

    let mut custom_out = JsonMap::new();
    let custom_keys: HashSet<&str> = custom_fields.iter().map(|f| f.key.as_str()).collect();
    for f in custom_fields {
        let raw = input.remove(&f.key);
        let value = coerce_custom(f, raw)?;
        if value.is_null() && f.required {
            return Err(CoreError::BadRequest(format!(
                "required custom field '{}' missing",
                f.key
            )));
        }
        if !value.is_null() {
            custom_out.insert(f.key.clone(), value);
        }
    }

    // Anything left in `input` is an unknown key. Drop silently — the embed
    // SDK shouldn't be able to OOM the DB by sending arbitrary payloads, and
    // returning a 400 for stray keys would be brittle as forms evolve.
    let _unknown: Vec<String> = input
        .into_keys()
        .filter(|k| !custom_keys.contains(k.as_str()))
        .collect();

    Ok((standard, JsonValue::Object(custom_out)))
}

fn coerce_custom(field: &CustomField, raw: Option<JsonValue>) -> CoreResult<JsonValue> {
    let raw = match raw {
        Some(JsonValue::Null) | None => return Ok(JsonValue::Null),
        Some(v) => v,
    };
    match &field.kind {
        CustomFieldType::Text
        | CustomFieldType::Email
        | CustomFieldType::Tel
        | CustomFieldType::Url
        | CustomFieldType::Textarea => match raw {
            JsonValue::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    Ok(JsonValue::Null)
                } else if trimmed.chars().count() > MAX_MESSAGE_LEN {
                    Err(CoreError::BadRequest(format!(
                        "custom field '{}' exceeds {MAX_MESSAGE_LEN} characters",
                        field.key
                    )))
                } else {
                    Ok(JsonValue::String(trimmed.to_string()))
                }
            }
            other => Err(CoreError::BadRequest(format!(
                "custom field '{}' must be a string, got {}",
                field.key,
                json_kind(&other)
            ))),
        },
        CustomFieldType::Number => match raw {
            JsonValue::Number(_) => Ok(raw),
            JsonValue::String(s) => {
                let t = s.trim();
                if t.is_empty() {
                    Ok(JsonValue::Null)
                } else {
                    t.parse::<f64>()
                        .ok()
                        .and_then(serde_json::Number::from_f64)
                        .map(JsonValue::Number)
                        .ok_or_else(|| {
                            CoreError::BadRequest(format!(
                                "custom field '{}' must be a number",
                                field.key
                            ))
                        })
                }
            }
            other => Err(CoreError::BadRequest(format!(
                "custom field '{}' must be a number, got {}",
                field.key,
                json_kind(&other)
            ))),
        },
        CustomFieldType::Checkbox => match raw {
            JsonValue::Bool(_) => Ok(raw),
            JsonValue::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                "" => Ok(JsonValue::Null),
                "true" | "on" | "yes" | "1" => Ok(JsonValue::Bool(true)),
                "false" | "off" | "no" | "0" => Ok(JsonValue::Bool(false)),
                _ => Err(CoreError::BadRequest(format!(
                    "custom field '{}' must be a boolean",
                    field.key
                ))),
            },
            other => Err(CoreError::BadRequest(format!(
                "custom field '{}' must be a boolean, got {}",
                field.key,
                json_kind(&other)
            ))),
        },
        CustomFieldType::Select { options } => match raw {
            JsonValue::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    Ok(JsonValue::Null)
                } else if options.iter().any(|opt| opt == trimmed) {
                    Ok(JsonValue::String(trimmed.to_string()))
                } else {
                    Err(CoreError::BadRequest(format!(
                        "custom field '{}' value '{trimmed}' is not one of the configured options",
                        field.key
                    )))
                }
            }
            JsonValue::Null => Ok(JsonValue::Null),
            other => Err(CoreError::BadRequest(format!(
                "custom field '{}' must be a string, got {}",
                field.key,
                json_kind(&other)
            ))),
        },
    }
}

fn json_kind(v: &JsonValue) -> &'static str {
    match v {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

/// Accept a submission for the given (already-loaded) form. Caller is
/// responsible for running this inside a transaction so the submission row
/// insert + the delivery fan-out commit atomically.
/// `true` if the honeypot field is present and non-empty (a bot filled the
/// hidden input a real user never sees).
fn honeypot_tripped(payload: &NewSubmissionPayload) -> bool {
    match payload.0.get(HONEYPOT_KEY) {
        None | Some(JsonValue::Null) => false,
        Some(JsonValue::String(s)) => !s.trim().is_empty(),
        Some(_) => true,
    }
}

/// Flatten the reserved `_source` object into string key/values. Non-string
/// scalars are stringified; nested objects/arrays/nulls are ignored. A missing
/// or non-object `_source` yields an empty map.
fn source_map_from(raw: Option<JsonValue>) -> HashMap<String, String> {
    let Some(JsonValue::Object(obj)) = raw else {
        return HashMap::new();
    };
    obj.into_iter()
        .filter_map(|(k, v)| match v {
            JsonValue::String(s) => Some((k, s)),
            JsonValue::Number(n) => Some((k, n.to_string())),
            JsonValue::Bool(b) => Some((k, b.to_string())),
            _ => None,
        })
        .collect()
}

/// Attribution derived from the QR landing page's source params: the resolved
/// rep id (if `?rep=<key>` matched one the form offers) and the subset of
/// declared source params that were present, ready to store on the submission.
struct Attribution {
    sales_rep_id: Option<i32>,
    source_params: Option<JsonValue>,
}

/// Resolve rep attribution + capture declared source params from `_source`.
async fn resolve_attribution<C: ConnectionTrait>(
    conn: &C,
    form: &entity::form::Model,
    source: &HashMap<String, String>,
) -> CoreResult<Attribution> {
    let sales_rep_id = match source.get(forms_service::REP_PARAM) {
        Some(key) => {
            let rep_ids = forms_service::reps_from_model(form)?;
            reps_service::resolve_for_form(conn, &rep_ids, key)
                .await?
                .map(|r| r.id)
        }
        None => None,
    };

    let declared = forms_service::source_params_from_model(form)?;
    let mut captured = JsonMap::new();
    for sp in &declared {
        if let Some(val) = source.get(&sp.param) {
            let trimmed = val.trim();
            if trimmed.is_empty() {
                continue;
            }
            let value: String = trimmed.chars().take(MAX_SOURCE_VALUE_LEN).collect();
            captured.insert(sp.param.clone(), JsonValue::String(value));
        }
    }
    let source_params = if captured.is_empty() {
        None
    } else {
        Some(JsonValue::Object(captured))
    };

    Ok(Attribution {
        sales_rep_id,
        source_params,
    })
}

pub async fn create_submission<C: ConnectionTrait>(
    conn: &C,
    form: &entity::form::Model,
    mut payload: NewSubmissionPayload,
) -> CoreResult<entity::submission::Model> {
    // Honeypot: reject (generically) if the hidden field was filled in.
    if honeypot_tripped(&payload) {
        return Err(CoreError::BadRequest("submission rejected".into()));
    }

    // Pull the reserved `_source` context (QR landing page query params) out of
    // the payload before field validation so it isn't mistaken for a form field.
    let source = source_map_from(payload.0.remove(SOURCE_KEY));

    // Read config straight off the JSON columns — no need to build a full
    // `FormDto` (which would also fetch metadata) on the hot submission path.
    let standard_cfg = forms_service::parse_standard_fields(&form.standard_fields)?;
    let custom_fields = forms_service::parse_custom_fields(&form.custom_fields)?;
    let backends = forms_service::backends_from_model(form)?;
    if backends.is_empty() {
        return Err(CoreError::Internal(anyhow!(
            "form {} has no backends configured",
            form.id
        )));
    }

    let attribution = resolve_attribution(conn, form, &source).await?;

    let (standard, custom_data) = validate_and_split(payload, &standard_cfg, &custom_fields)?;

    // Email deduplication (opt-in per form): if the submitted email already
    // exists on an earlier submission to this form, we still accept and store
    // the submission (the caller sees success) but flag it and skip the
    // delivery fan-out so it's never dispatched to a backend.
    let is_duplicate = is_duplicate_email(conn, form.id, standard.get("email")).await?;

    let inserted = insert_submission(
        conn,
        form.id,
        standard,
        custom_data,
        &attribution,
        is_duplicate,
    )
    .await?;
    if !is_duplicate {
        insert_deliveries(conn, inserted.id, &backends).await?;
    }
    Ok(inserted)
}

/// Whether this form has email deduplication enabled and `email` (already
/// trimmed by `validate_and_split`) matches an existing submission to the same
/// form. Returns `false` when dedup is off or no email was submitted.
///
/// Matching is case-insensitive: MySQL's default `utf8mb4` collation compares
/// strings case-insensitively, so `email = ?` treats `A@B.co` and `a@b.co` as
/// the same address — intentional for email dedup.
async fn is_duplicate_email<C: ConnectionTrait>(
    conn: &C,
    form_id: i32,
    email: Option<&String>,
) -> CoreResult<bool> {
    let Some(email) = email.filter(|e| !e.is_empty()) else {
        return Ok(false);
    };
    if !metadata_service::get_bool(conn, form_id, MetadataKey::EmailDeduplication)
        .await?
        .unwrap_or(false)
    {
        return Ok(false);
    }
    let existing = entity::submission::Entity::find()
        .filter(entity::submission::Column::FormId.eq(form_id))
        .filter(entity::submission::Column::Email.eq(email.as_str()))
        .one(conn)
        .await?;
    Ok(existing.is_some())
}

async fn insert_submission<C: ConnectionTrait>(
    conn: &C,
    form_id: i32,
    standard: StandardValues,
    custom_data: JsonValue,
    attribution: &Attribution,
    is_duplicate: bool,
) -> CoreResult<entity::submission::Model> {
    let take = |s: &StandardValues, key: &str| s.get(key).cloned();
    let active = entity::submission::ActiveModel {
        form_id: ActiveValue::Set(form_id),
        first_name: ActiveValue::Set(take(&standard, "first_name")),
        last_name: ActiveValue::Set(take(&standard, "last_name")),
        email: ActiveValue::Set(take(&standard, "email")),
        phone: ActiveValue::Set(take(&standard, "phone")),
        company: ActiveValue::Set(take(&standard, "company")),
        job_title: ActiveValue::Set(take(&standard, "job_title")),
        website: ActiveValue::Set(take(&standard, "website")),
        message: ActiveValue::Set(take(&standard, "message")),
        address_line_1: ActiveValue::Set(take(&standard, "address_line_1")),
        address_line_2: ActiveValue::Set(take(&standard, "address_line_2")),
        city: ActiveValue::Set(take(&standard, "city")),
        state: ActiveValue::Set(take(&standard, "state")),
        postal_code: ActiveValue::Set(take(&standard, "postal_code")),
        country: ActiveValue::Set(take(&standard, "country")),
        custom_data: ActiveValue::Set(custom_data),
        sales_rep_id: ActiveValue::Set(attribution.sales_rep_id),
        source_params: ActiveValue::Set(attribution.source_params.clone()),
        is_duplicate: ActiveValue::Set(Some(is_duplicate)),
        ..Default::default()
    };
    Ok(active.insert(conn).await?)
}

async fn insert_deliveries<C: ConnectionTrait>(
    conn: &C,
    submission_id: i32,
    backends: &[BackendBinding],
) -> CoreResult<()> {
    let now = Utc::now();
    for b in backends {
        let model = entity::submission_delivery::ActiveModel {
            submission_id: ActiveValue::Set(submission_id),
            backend_name: ActiveValue::Set(b.kind.clone()),
            backend_instance_id: ActiveValue::Set(b.instance_id),
            status: ActiveValue::Set(STATUS_PENDING.to_string()),
            attempts: ActiveValue::Set(0),
            next_attempt_at: ActiveValue::Set(now),
            last_error: ActiveValue::Set(None),
            delivered_at: ActiveValue::Set(None),
            ..Default::default()
        };
        model.insert(conn).await?;
    }
    Ok(())
}

pub async fn find_by_id<C: ConnectionTrait>(
    conn: &C,
    id: i32,
) -> CoreResult<Option<entity::submission::Model>> {
    Ok(entity::submission::Entity::find_by_id(id).one(conn).await?)
}

pub async fn list<C: ConnectionTrait>(conn: &C, q: &ListQuery) -> CoreResult<SubmissionList> {
    let limit = q.limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT);
    let offset = q.offset.unwrap_or(0);

    let mut select = entity::submission::Entity::find();
    if let Some(form_id) = q.form_id {
        select = select.filter(entity::submission::Column::FormId.eq(form_id));
    }

    let rows = select
        .clone()
        .order_by_desc(entity::submission::Column::Id)
        .limit(limit as u64)
        .offset(offset as u64)
        .all(conn)
        .await?;

    let mut items: Vec<SubmissionDto> = Vec::with_capacity(rows.len());
    let ids: Vec<i32> = rows.iter().map(|r| r.id).collect();
    let mut deliveries_by_sub = load_deliveries_for(conn, &ids).await?;
    for r in rows {
        let deliveries = deliveries_by_sub.remove(&r.id).unwrap_or_default();
        items.push(dto_from_model(r, deliveries));
    }

    let total = select.count(conn).await?;
    Ok(SubmissionList {
        items,
        total,
        limit,
        offset,
    })
}

pub async fn dto_for_id<C: ConnectionTrait>(
    conn: &C,
    id: i32,
) -> CoreResult<Option<SubmissionDto>> {
    let Some(row) = find_by_id(conn, id).await? else {
        return Ok(None);
    };
    let mut by_sub = load_deliveries_for(conn, &[row.id]).await?;
    let deliveries = by_sub.remove(&row.id).unwrap_or_default();
    Ok(Some(dto_from_model(row, deliveries)))
}

async fn load_deliveries_for<C: ConnectionTrait>(
    conn: &C,
    submission_ids: &[i32],
) -> CoreResult<HashMap<i32, Vec<SubmissionDeliveryDto>>> {
    let mut out: HashMap<i32, Vec<SubmissionDeliveryDto>> = HashMap::new();
    if submission_ids.is_empty() {
        return Ok(out);
    }
    let rows = entity::submission_delivery::Entity::find()
        .filter(entity::submission_delivery::Column::SubmissionId.is_in(submission_ids.to_vec()))
        .order_by_asc(entity::submission_delivery::Column::Id)
        .all(conn)
        .await?;
    for r in rows {
        out.entry(r.submission_id).or_default().push(delivery_dto(r));
    }
    Ok(out)
}

fn delivery_dto(m: entity::submission_delivery::Model) -> SubmissionDeliveryDto {
    SubmissionDeliveryDto {
        id: m.id,
        backend_name: m.backend_name,
        status: m.status,
        attempts: m.attempts,
        next_attempt_at: m.next_attempt_at,
        last_error: m.last_error,
        delivered_at: m.delivered_at,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

fn dto_from_model(
    m: entity::submission::Model,
    deliveries: Vec<SubmissionDeliveryDto>,
) -> SubmissionDto {
    SubmissionDto {
        id: m.id,
        form_id: m.form_id,
        first_name: m.first_name,
        last_name: m.last_name,
        email: m.email,
        phone: m.phone,
        company: m.company,
        job_title: m.job_title,
        website: m.website,
        message: m.message,
        address_line_1: m.address_line_1,
        address_line_2: m.address_line_2,
        city: m.city,
        state: m.state,
        postal_code: m.postal_code,
        country: m.country,
        custom_data: m.custom_data,
        sales_rep_id: m.sales_rep_id,
        source_params: m.source_params,
        is_duplicate: m.is_duplicate.unwrap_or(false),
        created_at: m.created_at,
        deliveries,
    }
}

/// Manually re-queue delivery rows for another attempt. For each requested id
/// we reset the row to a fresh `pending` attempt (cleared error, full retry
/// budget, due now) so the worker picks it up on its next poll. Rows already
/// `pending`/`in_progress` are skipped (re-queuing an in-flight row would be a
/// no-op at best, a double-send at worst); unknown ids are reported back.
///
/// `Backend::deliver` is contractually idempotent on `submission_id`, so
/// re-sending an already-`succeeded` row is safe.
pub async fn retry_deliveries<C: ConnectionTrait>(
    conn: &C,
    req: &RetryDeliveriesRequest,
) -> CoreResult<RetryDeliveriesResponse> {
    let now = Utc::now();
    let mut requeued = Vec::new();
    let mut skipped = Vec::new();
    let mut not_found = Vec::new();

    for &id in &req.delivery_ids {
        let Some(row) = entity::submission_delivery::Entity::find_by_id(id)
            .one(conn)
            .await?
        else {
            not_found.push(id);
            continue;
        };
        if row.status == STATUS_PENDING || row.status == STATUS_IN_PROGRESS {
            skipped.push(id);
            continue;
        }
        let mut active: entity::submission_delivery::ActiveModel = row.into();
        active.status = ActiveValue::Set(STATUS_PENDING.to_string());
        active.attempts = ActiveValue::Set(0);
        active.next_attempt_at = ActiveValue::Set(now);
        active.last_error = ActiveValue::Set(None);
        active.delivered_at = ActiveValue::Set(None);
        active.update(conn).await?;
        requeued.push(id);
    }

    Ok(RetryDeliveriesResponse {
        requeued,
        skipped,
        not_found,
    })
}

/// Build the JSON payload handed to a backend. Merges the standard columns
/// and `custom_data` into one flat object keyed by field key. This is the
/// shape `Backend::deliver` callers should rely on.
pub fn delivery_data(m: &entity::submission::Model) -> JsonValue {
    let mut obj = JsonMap::new();
    let push = |obj: &mut JsonMap<String, JsonValue>, key: &str, v: &Option<String>| {
        if let Some(value) = v {
            obj.insert(key.to_string(), JsonValue::String(value.clone()));
        }
    };
    push(&mut obj, "first_name", &m.first_name);
    push(&mut obj, "last_name", &m.last_name);
    push(&mut obj, "email", &m.email);
    push(&mut obj, "phone", &m.phone);
    push(&mut obj, "company", &m.company);
    push(&mut obj, "job_title", &m.job_title);
    push(&mut obj, "website", &m.website);
    push(&mut obj, "message", &m.message);
    push(&mut obj, "address_line_1", &m.address_line_1);
    push(&mut obj, "address_line_2", &m.address_line_2);
    push(&mut obj, "city", &m.city);
    push(&mut obj, "state", &m.state);
    push(&mut obj, "postal_code", &m.postal_code);
    push(&mut obj, "country", &m.country);
    if let JsonValue::Object(custom) = &m.custom_data {
        for (k, v) in custom {
            obj.insert(k.clone(), v.clone());
        }
    }
    JsonValue::Object(obj)
}

pub async fn delete_submission<C: ConnectionTrait>(conn: &C, id: i32) -> CoreResult<()> {
    entity::submission_delivery::Entity::delete_many()
        .filter(entity::submission_delivery::Column::SubmissionId.eq(id))
        .exec(conn)
        .await?;
    let res = entity::submission::Entity::delete_by_id(id).exec(conn).await?;
    if res.rows_affected == 0 {
        return Err(CoreError::NotFound("submission not found".into()));
    }
    Ok(())
}

/// Cascade hook for form deletion. Drops every submission tied to `form_id`
/// and every delivery row attached to those submissions. MUST be called from
/// `forms::service::delete_form` inside the same transaction.
pub async fn delete_for_form<C: ConnectionTrait>(conn: &C, form_id: i32) -> CoreResult<()> {
    let submission_ids: Vec<i32> = entity::submission::Entity::find()
        .filter(entity::submission::Column::FormId.eq(form_id))
        .select_only()
        .column(entity::submission::Column::Id)
        .into_tuple()
        .all(conn)
        .await?;
    if submission_ids.is_empty() {
        return Ok(());
    }
    entity::submission_delivery::Entity::delete_many()
        .filter(entity::submission_delivery::Column::SubmissionId.is_in(submission_ids))
        .exec(conn)
        .await?;
    entity::submission::Entity::delete_many()
        .filter(entity::submission::Column::FormId.eq(form_id))
        .exec(conn)
        .await?;
    Ok(())
}

/// Cascade hook for user deletion. Drops every submission tied to a form
/// owned by `user_id`. Run before `forms::service::delete_for_owner` so the
/// child rows are gone by the time the form rows disappear.
pub async fn delete_for_owner<C: ConnectionTrait>(conn: &C, user_id: i32) -> CoreResult<()> {
    let form_ids: Vec<i32> = entity::form::Entity::find()
        .filter(entity::form::Column::OwnerId.eq(user_id))
        .select_only()
        .column(entity::form::Column::Id)
        .into_tuple()
        .all(conn)
        .await?;
    for form_id in form_ids {
        delete_for_form(conn, form_id).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forms::{
        CustomField, CustomFieldType, StandardFieldConfig, StandardFieldsConfig,
    };

    #[test]
    fn honeypot_detection() {
        let trip = |v: serde_json::Value| {
            let mut m = std::collections::HashMap::new();
            m.insert(HONEYPOT_KEY.to_string(), v);
            honeypot_tripped(&NewSubmissionPayload(m))
        };
        assert!(trip(serde_json::json!("i am a bot")));
        assert!(trip(serde_json::json!(123)));
        assert!(!trip(serde_json::json!("")));
        assert!(!trip(serde_json::json!("   ")));
        assert!(!trip(serde_json::Value::Null));
        // Absent field => not tripped.
        assert!(!honeypot_tripped(&NewSubmissionPayload(std::collections::HashMap::new())));
    }

    fn enabled_required() -> StandardFieldConfig {
        StandardFieldConfig {
            enabled: true,
            required: true,
            label: None,
        }
    }
    fn enabled_optional() -> StandardFieldConfig {
        StandardFieldConfig {
            enabled: true,
            required: false,
            label: None,
        }
    }
    fn disabled() -> StandardFieldConfig {
        StandardFieldConfig {
            enabled: false,
            required: false,
            label: None,
        }
    }

    fn cfg() -> StandardFieldsConfig {
        StandardFieldsConfig {
            first_name: enabled_required(),
            last_name: enabled_optional(),
            email: enabled_required(),
            phone: disabled(),
            company: disabled(),
            job_title: disabled(),
            website: disabled(),
            message: enabled_optional(),
            address_line_1: disabled(),
            address_line_2: disabled(),
            city: disabled(),
            state: disabled(),
            postal_code: disabled(),
            country: disabled(),
        }
    }

    fn payload(pairs: &[(&str, JsonValue)]) -> NewSubmissionPayload {
        NewSubmissionPayload(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn rejects_missing_required_standard_field() {
        let res = validate_and_split(
            payload(&[("email", JsonValue::String("a@b.co".into()))]),
            &cfg(),
            &[],
        );
        assert!(matches!(res, Err(CoreError::BadRequest(_))));
    }

    #[test]
    fn drops_disabled_standard_field() {
        let (std, custom) = validate_and_split(
            payload(&[
                ("first_name", JsonValue::String("Ada".into())),
                ("email", JsonValue::String("a@b.co".into())),
                ("phone", JsonValue::String("555".into())),
            ]),
            &cfg(),
            &[],
        )
        .unwrap();
        assert_eq!(std.get("first_name"), Some(&"Ada".to_string()));
        assert!(!std.contains_key("phone"));
        assert!(custom.as_object().unwrap().is_empty());
    }

    #[test]
    fn parses_custom_select_against_options() {
        let fields = vec![CustomField {
            key: "color".into(),
            label: "Color".into(),
            kind: CustomFieldType::Select {
                options: vec!["red".into(), "blue".into()],
            },
            required: true,
            placeholder: None,
            help_text: None,
            position: 0,
        }];
        let (_std, custom) = validate_and_split(
            payload(&[
                ("first_name", JsonValue::String("A".into())),
                ("email", JsonValue::String("a@b.co".into())),
                ("color", JsonValue::String("red".into())),
            ]),
            &cfg(),
            &fields,
        )
        .unwrap();
        assert_eq!(custom["color"], JsonValue::String("red".into()));

        let bad = validate_and_split(
            payload(&[
                ("first_name", JsonValue::String("A".into())),
                ("email", JsonValue::String("a@b.co".into())),
                ("color", JsonValue::String("purple".into())),
            ]),
            &cfg(),
            &fields,
        );
        assert!(matches!(bad, Err(CoreError::BadRequest(_))));
    }

    #[test]
    fn source_map_flattens_scalars_and_ignores_nesting() {
        let raw = serde_json::json!({
            "rep": "jane",
            "event": "mjbiz-2026",
            "count": 3,
            "flag": true,
            "nested": { "a": 1 },
            "list": [1, 2],
        });
        let m = source_map_from(Some(raw));
        assert_eq!(m.get("rep").map(String::as_str), Some("jane"));
        assert_eq!(m.get("event").map(String::as_str), Some("mjbiz-2026"));
        assert_eq!(m.get("count").map(String::as_str), Some("3"));
        assert_eq!(m.get("flag").map(String::as_str), Some("true"));
        assert!(!m.contains_key("nested"));
        assert!(!m.contains_key("list"));
    }

    #[test]
    fn source_map_empty_for_non_object() {
        assert!(source_map_from(None).is_empty());
        assert!(source_map_from(Some(serde_json::json!("x"))).is_empty());
    }

    #[test]
    fn coerces_checkbox_strings() {
        let fields = vec![CustomField {
            key: "subscribe".into(),
            label: "Subscribe".into(),
            kind: CustomFieldType::Checkbox,
            required: false,
            placeholder: None,
            help_text: None,
            position: 0,
        }];
        let (_std, custom) = validate_and_split(
            payload(&[
                ("first_name", JsonValue::String("A".into())),
                ("email", JsonValue::String("a@b.co".into())),
                ("subscribe", JsonValue::String("on".into())),
            ]),
            &cfg(),
            &fields,
        )
        .unwrap();
        assert_eq!(custom["subscribe"], JsonValue::Bool(true));
    }
}
