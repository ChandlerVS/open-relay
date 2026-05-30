//! Persistence + validation for configurable backend instances.

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder,
};

use super::{
    BackendInstanceDto, BackendInstanceFormRef, BackendInstanceInUse, BackendInstanceList,
    NewBackendInstance, UpdateBackendInstance,
};
use crate::backend::{BackendBuildError, BackendRegistry};
use crate::error::{CoreError, CoreResult};

const MAX_NAME_LEN: usize = 200;

fn validate_name(name: &str) -> CoreResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_NAME_LEN {
        return Err(CoreError::BadRequest(format!(
            "name must be 1..={MAX_NAME_LEN} characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// Run the candidate config through the matching factory so a config the
/// runtime can't load never lands in the DB.
fn validate_config(
    registry: &BackendRegistry,
    kind: &str,
    config: &serde_json::Value,
) -> CoreResult<()> {
    let factory = registry
        .get_factory(kind)
        .ok_or_else(|| CoreError::BadRequest(format!("backend '{kind}' is not configurable")))?;
    match factory.build(config) {
        Ok(_) => Ok(()),
        Err(BackendBuildError::Invalid(msg)) => Err(CoreError::BadRequest(format!(
            "invalid backend config: {msg}"
        ))),
    }
}

pub async fn list<C: ConnectionTrait>(conn: &C) -> CoreResult<BackendInstanceList> {
    let rows = entity::backend_instance::Entity::find()
        .order_by_asc(entity::backend_instance::Column::Name)
        .all(conn)
        .await?;
    let total = rows.len() as u64;
    let items: Vec<BackendInstanceDto> = rows.into_iter().map(Into::into).collect();
    Ok(BackendInstanceList { items, total })
}

pub async fn find_by_id<C: ConnectionTrait>(
    conn: &C,
    id: i32,
) -> CoreResult<Option<entity::backend_instance::Model>> {
    Ok(entity::backend_instance::Entity::find_by_id(id)
        .one(conn)
        .await?)
}

pub async fn create<C: ConnectionTrait>(
    conn: &C,
    registry: &BackendRegistry,
    input: NewBackendInstance,
) -> CoreResult<entity::backend_instance::Model> {
    let kind = input.kind.trim().to_string();
    if !registry.is_configurable(&kind) {
        return Err(CoreError::BadRequest(format!(
            "backend kind '{kind}' is not configurable"
        )));
    }
    let name = validate_name(&input.name)?;
    validate_config(registry, &kind, &input.config)?;

    let active = entity::backend_instance::ActiveModel {
        kind: ActiveValue::Set(kind),
        name: ActiveValue::Set(name),
        config: ActiveValue::Set(input.config),
        ..Default::default()
    };
    Ok(active.insert(conn).await?)
}

pub async fn update<C: ConnectionTrait>(
    conn: &C,
    registry: &BackendRegistry,
    id: i32,
    input: UpdateBackendInstance,
) -> CoreResult<entity::backend_instance::Model> {
    let existing = find_by_id(conn, id)
        .await?
        .ok_or_else(|| CoreError::NotFound("backend instance not found".into()))?;
    let mut active: entity::backend_instance::ActiveModel = existing.clone().into();

    if let Some(name_raw) = input.name {
        let name = validate_name(&name_raw)?;
        active.name = ActiveValue::Set(name);
    }

    if let Some(config) = input.config {
        validate_config(registry, &existing.kind, &config)?;
        active.config = ActiveValue::Set(config);
    }

    Ok(active.update(conn).await?)
}

/// Delete a backend instance. Returns `CoreError::Conflict` (carrying a JSON
/// list of referencing forms in its message) if any `form.backends` row
/// still mentions this instance — the admin needs to unbind it first.
pub async fn delete<C: ConnectionTrait>(conn: &C, id: i32) -> CoreResult<()> {
    let existing = find_by_id(conn, id)
        .await?
        .ok_or_else(|| CoreError::NotFound("backend instance not found".into()))?;

    let refs = references_for(conn, &existing.kind, id).await?;
    if !refs.is_empty() {
        let in_use = BackendInstanceInUse { forms: refs };
        // Conflict carries a JSON-encoded payload so the HTTP layer can lift
        // it into the 409 response body. Keeping it as a serialized string
        // means `CoreError` stays framework-agnostic.
        let payload = serde_json::to_string(&in_use).unwrap_or_else(|_| "{}".to_string());
        return Err(CoreError::Conflict(payload));
    }

    entity::backend_instance::Entity::delete_by_id(id)
        .exec(conn)
        .await?;
    Ok(())
}

/// Scan every form's `backends` JSON column and return the ones that include
/// `{ kind, instance_id }`. Form counts are small in this app (hundreds, not
/// millions) so a full scan + in-memory parse is fine and avoids
/// vendor-specific `JSON_CONTAINS` syntax.
async fn references_for<C: ConnectionTrait>(
    conn: &C,
    kind: &str,
    instance_id: i32,
) -> CoreResult<Vec<BackendInstanceFormRef>> {
    let rows = entity::form::Entity::find()
        .filter(entity::form::Column::Backends.is_not_null())
        .all(conn)
        .await?;
    let mut out = Vec::new();
    for r in rows {
        let Some(raw) = &r.backends else { continue };
        let bindings: Vec<crate::forms::BackendBinding> = match serde_json::from_value(raw.clone()) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let referenced = bindings
            .iter()
            .any(|b| b.kind == kind && b.instance_id == Some(instance_id));
        if referenced {
            out.push(BackendInstanceFormRef {
                id: r.id,
                name: r.name,
                slug: r.slug,
            });
        }
    }
    Ok(out)
}

/// Cheap convenience for the worker / debug.
pub async fn count<C: ConnectionTrait>(conn: &C) -> CoreResult<u64> {
    Ok(entity::backend_instance::Entity::find().count(conn).await?)
}
