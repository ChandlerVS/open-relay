//! Sales rep persistence + validation.
//!
//! All functions take `&impl ConnectionTrait` so callers can pass either a
//! `DatabaseConnection` or a `DatabaseTransaction`.

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder,
};

use super::{NewRep, RepDto, RepList, UpdateRep};
use crate::error::{CoreError, CoreResult};
use crate::forms::service::{slugify, validate_slug};

const MAX_NAME_LEN: usize = 200;
const MAX_EMAIL_LEN: usize = 320;
const MAX_GHL_USER_ID_LEN: usize = 128;

fn validate_name(name: &str) -> CoreResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_NAME_LEN {
        return Err(CoreError::BadRequest(format!(
            "name must be 1..={MAX_NAME_LEN} characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// Rep keys share the form-slug grammar (lowercase ASCII letters/digits/hyphens)
/// so they're safe to drop into a `?rep=<key>` query string verbatim.
fn validate_key(key: &str) -> CoreResult<String> {
    validate_slug(key).map_err(|_| {
        CoreError::BadRequest(
            "key must be lowercase letters, digits, and hyphens (no leading/trailing/double hyphen)"
                .into(),
        )
    })
}

/// Trim an optional free-text field; an empty/blank string becomes `None`
/// (treated as "clear"). Rejects oversize values.
fn normalize_optional(
    field: &str,
    value: Option<String>,
    max: usize,
) -> CoreResult<Option<String>> {
    match value {
        None => Ok(None),
        Some(s) => {
            let t = s.trim();
            if t.is_empty() {
                Ok(None)
            } else if t.chars().count() > max {
                Err(CoreError::BadRequest(format!(
                    "{field} exceeds {max} characters"
                )))
            } else {
                Ok(Some(t.to_string()))
            }
        }
    }
}

pub async fn list<C: ConnectionTrait>(conn: &C) -> CoreResult<RepList> {
    let rows = entity::sales_rep::Entity::find()
        .order_by_asc(entity::sales_rep::Column::Name)
        .all(conn)
        .await?;
    let total = rows.len() as u64;
    let items: Vec<RepDto> = rows.into_iter().map(RepDto::from).collect();
    Ok(RepList { items, total })
}

pub async fn find_by_id<C: ConnectionTrait>(
    conn: &C,
    id: i32,
) -> CoreResult<Option<entity::sales_rep::Model>> {
    Ok(entity::sales_rep::Entity::find_by_id(id).one(conn).await?)
}

pub async fn find_by_key<C: ConnectionTrait>(
    conn: &C,
    key: &str,
) -> CoreResult<Option<entity::sales_rep::Model>> {
    Ok(entity::sales_rep::Entity::find()
        .filter(entity::sales_rep::Column::Key.eq(key))
        .one(conn)
        .await?)
}

/// Load every rep whose id is in `ids`, preserving the order of `ids`. Unknown
/// (e.g. since-deleted) ids are silently skipped — callers tolerate dangling
/// references in a form's `reps` list.
pub async fn list_by_ids<C: ConnectionTrait>(
    conn: &C,
    ids: &[i32],
) -> CoreResult<Vec<entity::sales_rep::Model>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = entity::sales_rep::Entity::find()
        .filter(entity::sales_rep::Column::Id.is_in(ids.to_vec()))
        .all(conn)
        .await?;
    let mut by_id: std::collections::HashMap<i32, entity::sales_rep::Model> =
        rows.into_iter().map(|m| (m.id, m)).collect();
    Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
}

/// Of the given ids, return the subset that resolve to an existing rep row.
/// Used by form validation to reject associations with unknown reps.
pub async fn existing_ids<C: ConnectionTrait>(conn: &C, ids: &[i32]) -> CoreResult<Vec<i32>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let found: Vec<i32> = entity::sales_rep::Entity::find()
        .filter(entity::sales_rep::Column::Id.is_in(ids.to_vec()))
        .all(conn)
        .await?
        .into_iter()
        .map(|m| m.id)
        .collect();
    Ok(found)
}

/// Resolve a `?rep=<key>` value against the reps a form offers. Returns the rep
/// only if `key` matches one of `form_rep_ids`; unknown keys / ids resolve to
/// `None` so an arbitrary URL value can never attribute to an unrelated rep.
pub async fn resolve_for_form<C: ConnectionTrait>(
    conn: &C,
    form_rep_ids: &[i32],
    key: &str,
) -> CoreResult<Option<entity::sales_rep::Model>> {
    let key = key.trim();
    if key.is_empty() || form_rep_ids.is_empty() {
        return Ok(None);
    }
    Ok(entity::sales_rep::Entity::find()
        .filter(entity::sales_rep::Column::Id.is_in(form_rep_ids.to_vec()))
        .filter(entity::sales_rep::Column::Key.eq(key))
        .one(conn)
        .await?)
}

pub async fn create<C: ConnectionTrait>(
    conn: &C,
    input: NewRep,
) -> CoreResult<entity::sales_rep::Model> {
    let name = validate_name(&input.name)?;
    let key_input = input
        .key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| slugify(&name));
    let key = validate_key(&key_input)?;
    if find_by_key(conn, &key).await?.is_some() {
        return Err(CoreError::Conflict("rep key already in use".into()));
    }
    let email = normalize_optional("email", input.email, MAX_EMAIL_LEN)?;
    let ghl_user_id = normalize_optional("ghl_user_id", input.ghl_user_id, MAX_GHL_USER_ID_LEN)?;

    let active = entity::sales_rep::ActiveModel {
        key: ActiveValue::Set(key),
        name: ActiveValue::Set(name),
        email: ActiveValue::Set(email),
        ghl_user_id: ActiveValue::Set(ghl_user_id),
        ..Default::default()
    };
    Ok(active.insert(conn).await?)
}

pub async fn update<C: ConnectionTrait>(
    conn: &C,
    id: i32,
    input: UpdateRep,
) -> CoreResult<entity::sales_rep::Model> {
    let existing = find_by_id(conn, id)
        .await?
        .ok_or_else(|| CoreError::NotFound("rep not found".into()))?;
    let mut active: entity::sales_rep::ActiveModel = existing.clone().into();

    if let Some(name_raw) = input.name {
        active.name = ActiveValue::Set(validate_name(&name_raw)?);
    }
    if let Some(key_raw) = input.key {
        let key = validate_key(&key_raw)?;
        if key != existing.key {
            if let Some(other) = find_by_key(conn, &key).await? {
                if other.id != id {
                    return Err(CoreError::Conflict("rep key already in use".into()));
                }
            }
            active.key = ActiveValue::Set(key);
        }
    }
    if input.email.is_some() {
        active.email = ActiveValue::Set(normalize_optional("email", input.email, MAX_EMAIL_LEN)?);
    }
    if input.ghl_user_id.is_some() {
        active.ghl_user_id =
            ActiveValue::Set(normalize_optional("ghl_user_id", input.ghl_user_id, MAX_GHL_USER_ID_LEN)?);
    }

    Ok(active.update(conn).await?)
}

/// Delete a rep. Any submission attributed to it has its `sales_rep_id` nulled
/// first (the historical lead stays, just loses the now-dangling reference).
/// Form `reps` lists may still mention the id; the read path tolerates that.
pub async fn delete<C: ConnectionTrait>(conn: &C, id: i32) -> CoreResult<()> {
    let existing = find_by_id(conn, id).await?;
    if existing.is_none() {
        return Err(CoreError::NotFound("rep not found".into()));
    }
    entity::submission::Entity::update_many()
        .col_expr(
            entity::submission::Column::SalesRepId,
            sea_orm::sea_query::Expr::value(sea_orm::Value::Int(None)),
        )
        .filter(entity::submission::Column::SalesRepId.eq(id))
        .exec(conn)
        .await?;
    entity::sales_rep::Entity::delete_by_id(id).exec(conn).await?;
    Ok(())
}

/// Cheap convenience for the dashboard / debug.
pub async fn count<C: ConnectionTrait>(conn: &C) -> CoreResult<u64> {
    Ok(entity::sales_rep::Entity::find().count(conn).await?)
}
