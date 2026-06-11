//! Persistence for form metadata (the EAV store).
//!
//! Every function takes `&impl ConnectionTrait` so it composes inside a
//! transaction (as form writes do in `forms::service`). Type-checking of values
//! against their key happens here, on write.

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
};

use super::{MetadataEntry, MetadataKey, MetadataValue};
use crate::error::{CoreError, CoreResult};

/// Upsert a metadata value for `(form_id, key)`. Rejects a value whose type
/// doesn't match the key's declared [`MetadataKey::value_type`].
pub async fn set<C: ConnectionTrait>(
    conn: &C,
    form_id: i32,
    key: MetadataKey,
    value: MetadataValue,
) -> CoreResult<()> {
    if value.value_type() != key.value_type() {
        return Err(CoreError::BadRequest(format!(
            "metadata key '{}' expects a {:?} value",
            key.slug(),
            key.value_type()
        )));
    }
    let encoded = value.to_storage();
    let existing = entity::form_metadata::Entity::find_by_id((form_id, key.slug().to_string()))
        .one(conn)
        .await?;
    match existing {
        Some(row) => {
            let mut active: entity::form_metadata::ActiveModel = row.into();
            active.value = ActiveValue::Set(encoded);
            active.update(conn).await?;
        }
        None => {
            let active = entity::form_metadata::ActiveModel {
                form_id: ActiveValue::Set(form_id),
                key: ActiveValue::Set(key.slug().to_string()),
                value: ActiveValue::Set(encoded),
                ..Default::default()
            };
            active.insert(conn).await?;
        }
    }
    Ok(())
}

/// Read a single metadata value, decoded per the key's type. `None` if unset.
pub async fn get<C: ConnectionTrait>(
    conn: &C,
    form_id: i32,
    key: MetadataKey,
) -> CoreResult<Option<MetadataValue>> {
    let row = entity::form_metadata::Entity::find_by_id((form_id, key.slug().to_string()))
        .one(conn)
        .await?;
    match row {
        Some(row) => Ok(Some(MetadataValue::from_storage(key.value_type(), &row.value)?)),
        None => Ok(None),
    }
}

/// Convenience accessor for boolean keys. `None` if unset; the decoded `bool`
/// otherwise.
pub async fn get_bool<C: ConnectionTrait>(
    conn: &C,
    form_id: i32,
    key: MetadataKey,
) -> CoreResult<Option<bool>> {
    Ok(get(conn, form_id, key).await?.and_then(|v| v.as_bool()))
}

/// All known metadata for a form. Rows whose key no longer maps via
/// `from_slug` (a since-removed variant) are silently skipped.
pub async fn list<C: ConnectionTrait>(conn: &C, form_id: i32) -> CoreResult<Vec<MetadataEntry>> {
    let rows = entity::form_metadata::Entity::find()
        .filter(entity::form_metadata::Column::FormId.eq(form_id))
        .all(conn)
        .await?;
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        let Some(key) = MetadataKey::from_slug(&row.key) else {
            continue;
        };
        let value = MetadataValue::from_storage(key.value_type(), &row.value)?;
        entries.push(MetadataEntry { key, value });
    }
    Ok(entries)
}

/// Remove a single metadata value. No-op if it was unset.
pub async fn delete<C: ConnectionTrait>(
    conn: &C,
    form_id: i32,
    key: MetadataKey,
) -> CoreResult<()> {
    entity::form_metadata::Entity::delete_by_id((form_id, key.slug().to_string()))
        .exec(conn)
        .await?;
    Ok(())
}

/// Cascade hook for form deletion. Removes every metadata row for `form_id`.
/// FK cleanup is in application code (no DB cascade), so this MUST be called
/// from `forms::service::delete_form` inside the same transaction.
pub async fn delete_for_form<C: ConnectionTrait>(conn: &C, form_id: i32) -> CoreResult<()> {
    entity::form_metadata::Entity::delete_many()
        .filter(entity::form_metadata::Column::FormId.eq(form_id))
        .exec(conn)
        .await?;
    Ok(())
}
