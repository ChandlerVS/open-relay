//! Form persistence + validation.
//!
//! All functions take `&impl ConnectionTrait` so callers can pass either
//! a `DatabaseConnection` or a `DatabaseTransaction`.

use std::collections::HashSet;

use anyhow::anyhow;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect,
};

use super::{
    BackendBinding, CustomField, CustomFieldType, FormDto, FormList, FormSelectOption, ListQuery,
    NewForm, PublicFormDto, STANDARD_FIELD_KEYS, StandardFieldsConfig, UpdateForm,
    default_backends,
};
use crate::backend::BackendRegistry;
use crate::error::{CoreError, CoreResult};

const MAX_NAME_LEN: usize = 200;
const MAX_SLUG_LEN: usize = 100;
const MAX_LABEL_LEN: usize = 200;
const MAX_KEY_LEN: usize = 64;
const MAX_CUSTOM_FIELDS: usize = 100;
const DEFAULT_LIST_LIMIT: u32 = 50;
const MAX_LIST_LIMIT: u32 = 200;

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
        if let CustomFieldType::Select { options } = &f.kind {
            if options.is_empty() {
                return Err(CoreError::BadRequest(format!(
                    "custom field '{}' is a select but has no options",
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

fn parse_standard_fields(value: &sea_orm::JsonValue) -> CoreResult<StandardFieldsConfig> {
    serde_json::from_value(value.clone())
        .map_err(|e| CoreError::Internal(anyhow!("failed to parse standard_fields json: {e}")))
}

fn parse_custom_fields(value: &sea_orm::JsonValue) -> CoreResult<Vec<CustomField>> {
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
/// columns into their typed shapes.
pub fn dto_from_model(m: entity::form::Model) -> CoreResult<FormDto> {
    let standard_fields = parse_standard_fields(&m.standard_fields)?;
    let custom_fields = parse_custom_fields(&m.custom_fields)?;
    let backends = backends_from_model(&m)?;
    Ok(FormDto {
        id: m.id,
        owner_id: m.owner_id,
        name: m.name,
        slug: m.slug,
        standard_fields,
        custom_fields,
        backends,
        created_at: m.created_at,
        updated_at: m.updated_at,
    })
}

pub fn public_dto_from_model(m: entity::form::Model) -> CoreResult<PublicFormDto> {
    let standard_fields = parse_standard_fields(&m.standard_fields)?;
    let custom_fields = parse_custom_fields(&m.custom_fields)?;
    let backends = backends_from_model(&m)?;
    Ok(PublicFormDto {
        id: m.id,
        name: m.name,
        slug: m.slug,
        standard_fields,
        custom_fields,
        backends,
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

    let standard_fields = input.standard_fields.unwrap_or_default();
    let custom_fields = normalize_custom_fields(input.custom_fields);
    validate_custom_fields(&custom_fields)?;

    let backends = input.backends.unwrap_or_else(default_backends);
    validate_backends(conn, &backends, registry).await?;

    let model = entity::form::ActiveModel {
        owner_id: ActiveValue::Set(owner_id),
        name: ActiveValue::Set(name),
        slug: ActiveValue::Set(slug),
        standard_fields: ActiveValue::Set(json_or_internal(&standard_fields)?),
        custom_fields: ActiveValue::Set(json_or_internal(&custom_fields)?),
        backends: ActiveValue::Set(Some(json_or_internal(&backends)?)),
        ..Default::default()
    };
    Ok(model.insert(conn).await?)
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
        items.push(dto_from_model(r)?);
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

    if let Some(sf) = input.standard_fields {
        active.standard_fields = ActiveValue::Set(json_or_internal(&sf)?);
    }

    if let Some(cf) = input.custom_fields {
        let cf = normalize_custom_fields(cf);
        validate_custom_fields(&cf)?;
        active.custom_fields = ActiveValue::Set(json_or_internal(&cf)?);
    }

    if let Some(b) = input.backends {
        validate_backends(conn, &b, registry).await?;
        active.backends = ActiveValue::Set(Some(json_or_internal(&b)?));
    }

    Ok(active.update(conn).await?)
}

pub async fn delete_form<C: ConnectionTrait>(conn: &C, id: i32) -> CoreResult<()> {
    crate::submissions::service::delete_for_form(conn, id).await?;
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
            },
            CustomField {
                key: "shoe_size".into(),
                label: "Again".into(),
                kind: CustomFieldType::Text,
                required: false,
                placeholder: None,
                help_text: None,
                position: 1,
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
        }];
        assert!(validate_custom_fields(&fields).is_err());
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
        }];
        assert!(validate_custom_fields(&fields).is_err());
    }
}
