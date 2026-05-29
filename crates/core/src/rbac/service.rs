//! Role + assignment service.
//!
//! All public functions take `&impl ConnectionTrait` so callers can pass a
//! plain `DatabaseConnection` or a `DatabaseTransaction`. Functions that
//! enforce the "don't lock yourselves out" invariants (`assign_roles_to_user`,
//! and `users::service::delete_user`) MUST be called from inside a
//! transaction — the `lock_exclusive()` on the superadmin role row only
//! holds for the duration of the enclosing tx. Callers that don't already
//! wrap in a tx will leak the lock immediately on MySQL.

use std::collections::{HashMap, HashSet};

use anyhow::anyhow;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
};

use super::{NewRole, RoleDto, RoleSummary, UpdateRole};
use crate::error::{CoreError, CoreResult};
use crate::permissions::Permission;

const MAX_ROLE_NAME_LEN: usize = 255;
const MAX_ROLE_DESCRIPTION_LEN: usize = 1024;

// ----------------------------------------------------------------------------
// Lookup
// ----------------------------------------------------------------------------

pub async fn list_roles<C: ConnectionTrait>(conn: &C) -> CoreResult<Vec<RoleDto>> {
    let roles = entity::role::Entity::find()
        .order_by_desc(entity::role::Column::IsSystem)
        .order_by_asc(entity::role::Column::Name)
        .all(conn)
        .await?;
    let mut perms_by_role = permissions_by_role(conn, roles.iter().map(|r| r.id).collect()).await?;
    Ok(roles
        .into_iter()
        .map(|r| RoleDto {
            id: r.id,
            name: r.name,
            description: r.description,
            is_system: r.is_system,
            permissions: perms_by_role.remove(&r.id).unwrap_or_default(),
        })
        .collect())
}

pub async fn get_role<C: ConnectionTrait>(conn: &C, id: i32) -> CoreResult<RoleDto> {
    let role = entity::role::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| CoreError::NotFound("role not found".into()))?;
    let perms = permissions_for_role(conn, id).await?;
    Ok(RoleDto {
        id: role.id,
        name: role.name,
        description: role.description,
        is_system: role.is_system,
        permissions: perms,
    })
}

pub async fn select_list_summary<C: ConnectionTrait>(conn: &C) -> CoreResult<Vec<RoleSummary>> {
    Ok(entity::role::Entity::find()
        .order_by_desc(entity::role::Column::IsSystem)
        .order_by_asc(entity::role::Column::Name)
        .all(conn)
        .await?
        .into_iter()
        .map(Into::into)
        .collect())
}

pub async fn roles_for_user<C: ConnectionTrait>(
    conn: &C,
    user_id: i32,
) -> CoreResult<Vec<RoleSummary>> {
    let role_ids: Vec<i32> = entity::user_role::Entity::find()
        .filter(entity::user_role::Column::UserId.eq(user_id))
        .all(conn)
        .await?
        .into_iter()
        .map(|ur| ur.role_id)
        .collect();
    if role_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(entity::role::Entity::find()
        .filter(entity::role::Column::Id.is_in(role_ids))
        .order_by_desc(entity::role::Column::IsSystem)
        .order_by_asc(entity::role::Column::Name)
        .all(conn)
        .await?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// Batched variant — one assignment query, one role query — used by the
/// users list endpoint to avoid N+1 fetches.
pub async fn roles_for_users<C: ConnectionTrait>(
    conn: &C,
    user_ids: &[i32],
) -> CoreResult<HashMap<i32, Vec<RoleSummary>>> {
    let mut out: HashMap<i32, Vec<RoleSummary>> = HashMap::new();
    if user_ids.is_empty() {
        return Ok(out);
    }
    let assignments = entity::user_role::Entity::find()
        .filter(entity::user_role::Column::UserId.is_in(user_ids.iter().copied()))
        .all(conn)
        .await?;
    let role_ids: HashSet<i32> = assignments.iter().map(|ur| ur.role_id).collect();
    if role_ids.is_empty() {
        return Ok(out);
    }
    let roles: HashMap<i32, RoleSummary> = entity::role::Entity::find()
        .filter(entity::role::Column::Id.is_in(role_ids))
        .all(conn)
        .await?
        .into_iter()
        .map(|r| (r.id, r.into()))
        .collect();
    for ur in assignments {
        if let Some(summary) = roles.get(&ur.role_id) {
            out.entry(ur.user_id).or_default().push(summary.clone());
        }
    }
    for entries in out.values_mut() {
        entries.sort_by(|a, b| {
            b.is_system
                .cmp(&a.is_system)
                .then_with(|| a.name.cmp(&b.name))
        });
    }
    Ok(out)
}

/// Flat set of permissions a user holds, expanded from their role grants.
/// Unknown slugs (left over from a previously-deployed enum variant) are
/// silently dropped — see the doc on `Permission::from_slug`.
pub async fn load_user_permissions<C: ConnectionTrait>(
    conn: &C,
    user_id: i32,
) -> CoreResult<HashSet<Permission>> {
    let role_ids: Vec<i32> = entity::user_role::Entity::find()
        .filter(entity::user_role::Column::UserId.eq(user_id))
        .all(conn)
        .await?
        .into_iter()
        .map(|ur| ur.role_id)
        .collect();
    if role_ids.is_empty() {
        return Ok(HashSet::new());
    }
    let rows = entity::role_permission::Entity::find()
        .filter(entity::role_permission::Column::RoleId.is_in(role_ids))
        .all(conn)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|rp| Permission::from_slug(&rp.permission))
        .collect())
}

// ----------------------------------------------------------------------------
// Mutations
// ----------------------------------------------------------------------------

pub async fn create_role<C: ConnectionTrait>(conn: &C, input: NewRole) -> CoreResult<RoleDto> {
    let name = input.name.trim().to_string();
    validate_name(&name)?;
    let description = normalize_description(input.description.as_deref())?;
    if name_exists(conn, &name, None).await? {
        return Err(CoreError::Conflict("role name already in use".into()));
    }
    let model = entity::role::ActiveModel {
        name: ActiveValue::Set(name),
        description: ActiveValue::Set(description),
        is_system: ActiveValue::Set(false),
        ..Default::default()
    };
    let inserted = model.insert(conn).await?;

    let mut perms = input.permissions.clone();
    perms.sort_by_key(|p| p.slug());
    perms.dedup();
    insert_permissions(conn, inserted.id, &perms).await?;

    Ok(RoleDto {
        id: inserted.id,
        name: inserted.name,
        description: inserted.description,
        is_system: inserted.is_system,
        permissions: perms,
    })
}

pub async fn update_role<C: ConnectionTrait>(
    conn: &C,
    id: i32,
    input: UpdateRole,
) -> CoreResult<RoleDto> {
    let existing = entity::role::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| CoreError::NotFound("role not found".into()))?;
    if existing.is_system {
        return Err(CoreError::Forbidden("system role cannot be modified".into()));
    }
    let mut active: entity::role::ActiveModel = existing.clone().into();

    if let Some(name_raw) = input.name {
        let name = name_raw.trim().to_string();
        validate_name(&name)?;
        if name != existing.name {
            if name_exists(conn, &name, Some(id)).await? {
                return Err(CoreError::Conflict("role name already in use".into()));
            }
            active.name = ActiveValue::Set(name);
        }
    }
    if let Some(desc_raw) = input.description {
        let normalized = normalize_description(Some(&desc_raw))?;
        active.description = ActiveValue::Set(normalized);
    }
    active.update(conn).await?;

    if let Some(mut perms) = input.permissions {
        perms.sort_by_key(|p| p.slug());
        perms.dedup();
        replace_permissions(conn, id, &perms).await?;
    }

    get_role(conn, id).await
}

/// Deletes a role and all dependent rows (`role_permission`, `user_role`).
/// Rejects system roles. Caller must ensure the tx is open if they need
/// the cleanup to be atomic with anything else.
pub async fn delete_role<C: ConnectionTrait>(conn: &C, id: i32) -> CoreResult<()> {
    let existing = entity::role::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| CoreError::NotFound("role not found".into()))?;
    if existing.is_system {
        return Err(CoreError::Forbidden("system role cannot be deleted".into()));
    }
    entity::role_permission::Entity::delete_many()
        .filter(entity::role_permission::Column::RoleId.eq(id))
        .exec(conn)
        .await?;
    entity::user_role::Entity::delete_many()
        .filter(entity::user_role::Column::RoleId.eq(id))
        .exec(conn)
        .await?;
    entity::role::Entity::delete_by_id(id).exec(conn).await?;
    Ok(())
}

/// Replaces the user's role set with `role_ids`. Validates that every id
/// exists and rejects any change that would leave zero superadmin users
/// — the latter check acquires `lock_exclusive()` on the superadmin role
/// row so concurrent demotions of different users can't collude to lock
/// the system out.
///
/// MUST be called inside a transaction.
pub async fn assign_roles_to_user<C: ConnectionTrait>(
    conn: &C,
    user_id: i32,
    role_ids: &[i32],
    superadmin_role_id: i32,
) -> CoreResult<()> {
    let mut requested: Vec<i32> = role_ids.iter().copied().collect();
    requested.sort();
    requested.dedup();

    if !requested.is_empty() {
        let known = entity::role::Entity::find()
            .filter(entity::role::Column::Id.is_in(requested.iter().copied()))
            .count(conn)
            .await?;
        if (known as usize) != requested.len() {
            return Err(CoreError::BadRequest("unknown role id".into()));
        }
    }

    let current: HashSet<i32> = entity::user_role::Entity::find()
        .filter(entity::user_role::Column::UserId.eq(user_id))
        .all(conn)
        .await?
        .into_iter()
        .map(|ur| ur.role_id)
        .collect();
    let requested_set: HashSet<i32> = requested.iter().copied().collect();

    let removing_superadmin =
        current.contains(&superadmin_role_id) && !requested_set.contains(&superadmin_role_id);
    if removing_superadmin {
        guard_last_superadmin(conn, user_id, superadmin_role_id).await?;
    }

    let to_remove: Vec<i32> = current.difference(&requested_set).copied().collect();
    let to_add: Vec<i32> = requested_set.difference(&current).copied().collect();

    if !to_remove.is_empty() {
        entity::user_role::Entity::delete_many()
            .filter(entity::user_role::Column::UserId.eq(user_id))
            .filter(entity::user_role::Column::RoleId.is_in(to_remove))
            .exec(conn)
            .await?;
    }
    for role_id in to_add {
        entity::user_role::ActiveModel {
            user_id: ActiveValue::Set(user_id),
            role_id: ActiveValue::Set(role_id),
        }
        .insert(conn)
        .await?;
    }
    Ok(())
}

/// Idempotent boot routine: ensures the `Superadmin` role exists and that
/// its grants exactly match `Permission::all()`. Returns the role id.
///
/// Runs in its own transaction so the `lock_exclusive()` on the role row
/// covers the diff. Also sweeps non-superadmin roles for orphan slugs and
/// logs them as a `warn!` — useful for catching a renamed enum variant
/// that left dangling DB rows.
pub async fn ensure_superadmin(db: &DatabaseConnection) -> CoreResult<i32> {
    db.transaction::<_, i32, CoreError>(|tx| Box::pin(ensure_superadmin_inner(tx)))
        .await
        .map_err(unwrap_tx)
}

async fn ensure_superadmin_inner(
    tx: &sea_orm::DatabaseTransaction,
) -> CoreResult<i32> {
    let system_rows = entity::role::Entity::find()
        .filter(entity::role::Column::IsSystem.eq(true))
        .all(tx)
        .await?;
    if system_rows.len() > 1 {
        tracing::warn!(
            count = system_rows.len(),
            "multiple is_system roles found; using the first by id"
        );
    }
    let role_id = match system_rows.into_iter().min_by_key(|r| r.id) {
        Some(existing) => {
            entity::role::Entity::find_by_id(existing.id)
                .lock_exclusive()
                .one(tx)
                .await?
                .ok_or_else(|| CoreError::Internal(anyhow!("locked row vanished")))?;
            existing.id
        }
        None => {
            let now = Utc::now();
            let model = entity::role::ActiveModel {
                name: ActiveValue::Set("Superadmin".into()),
                description: ActiveValue::Set(Some(
                    "System-managed role with every permission. Auto-synced on boot.".into(),
                )),
                is_system: ActiveValue::Set(true),
                created_at: ActiveValue::Set(now),
                updated_at: ActiveValue::Set(now),
                ..Default::default()
            };
            let inserted = model.insert(tx).await?;
            entity::role::Entity::find_by_id(inserted.id)
                .lock_exclusive()
                .one(tx)
                .await?;
            inserted.id
        }
    };

    let want: HashSet<&'static str> = Permission::all().iter().map(|p| p.slug()).collect();
    let have_rows = entity::role_permission::Entity::find()
        .filter(entity::role_permission::Column::RoleId.eq(role_id))
        .all(tx)
        .await?;
    let have: HashSet<String> = have_rows.iter().map(|rp| rp.permission.clone()).collect();

    let to_remove: Vec<String> = have
        .iter()
        .filter(|s| !want.contains(s.as_str()))
        .cloned()
        .collect();
    let to_add: Vec<&'static str> = want
        .into_iter()
        .filter(|s| !have.contains(*s))
        .collect();

    if !to_remove.is_empty() {
        entity::role_permission::Entity::delete_many()
            .filter(entity::role_permission::Column::RoleId.eq(role_id))
            .filter(entity::role_permission::Column::Permission.is_in(to_remove.clone()))
            .exec(tx)
            .await?;
        tracing::info!(?to_remove, role_id, "pruned obsolete permissions from superadmin");
    }
    for slug in &to_add {
        entity::role_permission::ActiveModel {
            role_id: ActiveValue::Set(role_id),
            permission: ActiveValue::Set((*slug).to_string()),
        }
        .insert(tx)
        .await?;
    }
    if !to_add.is_empty() {
        tracing::info!(?to_add, role_id, "added new permissions to superadmin");
    }

    let other_rows = entity::role_permission::Entity::find()
        .filter(entity::role_permission::Column::RoleId.ne(role_id))
        .all(tx)
        .await?;
    let orphans: Vec<String> = other_rows
        .into_iter()
        .filter(|rp| Permission::from_slug(&rp.permission).is_none())
        .map(|rp| rp.permission)
        .collect();
    if !orphans.is_empty() {
        tracing::warn!(
            ?orphans,
            "orphan permission slugs found in non-superadmin roles (renamed or removed?)"
        );
    }

    Ok(role_id)
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

fn validate_name(name: &str) -> CoreResult<()> {
    if name.is_empty() || name.len() > MAX_ROLE_NAME_LEN {
        return Err(CoreError::BadRequest(format!(
            "role name must be 1..={MAX_ROLE_NAME_LEN} characters"
        )));
    }
    Ok(())
}

fn normalize_description(d: Option<&str>) -> CoreResult<Option<String>> {
    let Some(raw) = d else { return Ok(None) };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() > MAX_ROLE_DESCRIPTION_LEN {
        return Err(CoreError::BadRequest(format!(
            "description must be ≤ {MAX_ROLE_DESCRIPTION_LEN} characters"
        )));
    }
    Ok(Some(trimmed.to_string()))
}

async fn name_exists<C: ConnectionTrait>(
    conn: &C,
    name: &str,
    exclude_id: Option<i32>,
) -> CoreResult<bool> {
    let mut q = entity::role::Entity::find().filter(entity::role::Column::Name.eq(name));
    if let Some(id) = exclude_id {
        q = q.filter(entity::role::Column::Id.ne(id));
    }
    Ok(q.one(conn).await?.is_some())
}

async fn permissions_for_role<C: ConnectionTrait>(
    conn: &C,
    role_id: i32,
) -> CoreResult<Vec<Permission>> {
    let mut perms: Vec<Permission> = entity::role_permission::Entity::find()
        .filter(entity::role_permission::Column::RoleId.eq(role_id))
        .all(conn)
        .await?
        .into_iter()
        .filter_map(|rp| Permission::from_slug(&rp.permission))
        .collect();
    perms.sort_by_key(|p| p.slug());
    Ok(perms)
}

async fn permissions_by_role<C: ConnectionTrait>(
    conn: &C,
    role_ids: Vec<i32>,
) -> CoreResult<HashMap<i32, Vec<Permission>>> {
    let mut out: HashMap<i32, Vec<Permission>> = HashMap::new();
    if role_ids.is_empty() {
        return Ok(out);
    }
    let rows = entity::role_permission::Entity::find()
        .filter(entity::role_permission::Column::RoleId.is_in(role_ids))
        .all(conn)
        .await?;
    for rp in rows {
        if let Some(p) = Permission::from_slug(&rp.permission) {
            out.entry(rp.role_id).or_default().push(p);
        }
    }
    for v in out.values_mut() {
        v.sort_by_key(|p| p.slug());
    }
    Ok(out)
}

async fn insert_permissions<C: ConnectionTrait>(
    conn: &C,
    role_id: i32,
    perms: &[Permission],
) -> CoreResult<()> {
    for p in perms {
        entity::role_permission::ActiveModel {
            role_id: ActiveValue::Set(role_id),
            permission: ActiveValue::Set(p.slug().to_string()),
        }
        .insert(conn)
        .await?;
    }
    Ok(())
}

async fn replace_permissions<C: ConnectionTrait>(
    conn: &C,
    role_id: i32,
    perms: &[Permission],
) -> CoreResult<()> {
    entity::role_permission::Entity::delete_many()
        .filter(entity::role_permission::Column::RoleId.eq(role_id))
        .exec(conn)
        .await?;
    insert_permissions(conn, role_id, perms).await
}

/// Acquires the row-level lock on the superadmin role and counts other
/// users still holding it. Errors with `Forbidden` if removing this user
/// would drain the set.
///
/// Both reads are locking on purpose. MySQL InnoDB's default isolation
/// (REPEATABLE READ) would otherwise serve the count from the tx's BEGIN
/// snapshot — letting two concurrent demotion transactions both observe
/// "another superadmin still exists" and commit, draining the set. The
/// `lock_exclusive()` on the role row serializes the critical section,
/// and the locking read on `user_role` forces a current-read so the
/// second transaction sees the first one's committed delete.
pub(crate) async fn guard_last_superadmin<C: ConnectionTrait>(
    conn: &C,
    user_id_being_changed: i32,
    superadmin_role_id: i32,
) -> CoreResult<()> {
    entity::role::Entity::find_by_id(superadmin_role_id)
        .lock_exclusive()
        .one(conn)
        .await?
        .ok_or_else(|| CoreError::Internal(anyhow!("superadmin role missing")))?;
    let others = entity::user_role::Entity::find()
        .filter(entity::user_role::Column::RoleId.eq(superadmin_role_id))
        .filter(entity::user_role::Column::UserId.ne(user_id_being_changed))
        .lock_exclusive()
        .all(conn)
        .await?
        .len();
    if others == 0 {
        return Err(CoreError::Forbidden(
            "would leave system without a superadmin".into(),
        ));
    }
    Ok(())
}

fn unwrap_tx(err: sea_orm::TransactionError<CoreError>) -> CoreError {
    match err {
        sea_orm::TransactionError::Connection(db) => CoreError::Db(db),
        sea_orm::TransactionError::Transaction(core) => core,
    }
}
