//! Persistence for the user ↔ external-identity link table.

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder,
};

use super::ExternalIdentityDto;
use crate::error::{CoreError, CoreResult};

pub async fn find_by_provider_subject<C: ConnectionTrait>(
    conn: &C,
    provider_config_id: i32,
    subject: &str,
) -> CoreResult<Option<entity::external_identity::Model>> {
    Ok(entity::external_identity::Entity::find()
        .filter(entity::external_identity::Column::ProviderConfigId.eq(provider_config_id))
        .filter(entity::external_identity::Column::Subject.eq(subject))
        .one(conn)
        .await?)
}

pub async fn list_for_user<C: ConnectionTrait>(
    conn: &C,
    user_id: i32,
) -> CoreResult<Vec<ExternalIdentityDto>> {
    let rows = entity::external_identity::Entity::find()
        .filter(entity::external_identity::Column::UserId.eq(user_id))
        .order_by_asc(entity::external_identity::Column::CreatedAt)
        .all(conn)
        .await?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let provider_ids: Vec<i32> = {
        let mut v: Vec<i32> = rows.iter().map(|r| r.provider_config_id).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let providers = entity::oauth_provider_config::Entity::find()
        .filter(entity::oauth_provider_config::Column::Id.is_in(provider_ids))
        .all(conn)
        .await?;
    let name_for = |id: i32| -> String {
        providers
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.display_name.clone())
            .unwrap_or_else(|| "(removed)".into())
    };
    Ok(rows
        .into_iter()
        .map(|r| ExternalIdentityDto {
            id: r.id,
            provider_config_id: r.provider_config_id,
            provider_display_name: name_for(r.provider_config_id),
            email_at_link: r.email_at_link,
            created_at: r.created_at,
        })
        .collect())
}

pub async fn link_to_user<C: ConnectionTrait>(
    conn: &C,
    user_id: i32,
    provider_config_id: i32,
    subject: &str,
    email_at_link: Option<&str>,
) -> CoreResult<entity::external_identity::Model> {
    // Reject if another user already owns this (provider, subject).
    if let Some(existing) = find_by_provider_subject(conn, provider_config_id, subject).await? {
        if existing.user_id != user_id {
            return Err(CoreError::Conflict(
                "this provider identity is already linked to another user".into(),
            ));
        }
        return Ok(existing);
    }
    // Reject if this user is already linked to this provider.
    let already_linked = entity::external_identity::Entity::find()
        .filter(entity::external_identity::Column::UserId.eq(user_id))
        .filter(entity::external_identity::Column::ProviderConfigId.eq(provider_config_id))
        .one(conn)
        .await?;
    if already_linked.is_some() {
        return Err(CoreError::Conflict(
            "this user is already linked to this provider".into(),
        ));
    }

    let active = entity::external_identity::ActiveModel {
        user_id: ActiveValue::Set(user_id),
        provider_config_id: ActiveValue::Set(provider_config_id),
        subject: ActiveValue::Set(subject.to_string()),
        email_at_link: ActiveValue::Set(email_at_link.map(|s| s.to_string())),
        ..Default::default()
    };
    Ok(active.insert(conn).await?)
}

/// Unlink a single identity belonging to `user_id`. Refuses to remove the
/// user's last identity if they have no local password (which would lock
/// them out).
pub async fn unlink<C: ConnectionTrait>(
    conn: &C,
    user_id: i32,
    identity_id: i32,
) -> CoreResult<()> {
    let identity = entity::external_identity::Entity::find_by_id(identity_id)
        .one(conn)
        .await?
        .ok_or_else(|| CoreError::NotFound("identity not found".into()))?;
    if identity.user_id != user_id {
        return Err(CoreError::NotFound("identity not found".into()));
    }

    let user = entity::user::Entity::find_by_id(user_id)
        .one(conn)
        .await?
        .ok_or_else(|| CoreError::NotFound("user not found".into()))?;
    let has_password = user.password_hash.is_some();
    if !has_password {
        let count = entity::external_identity::Entity::find()
            .filter(entity::external_identity::Column::UserId.eq(user_id))
            .count(conn)
            .await?;
        if count <= 1 {
            return Err(CoreError::Forbidden(
                "cannot unlink last identity without a local password".into(),
            ));
        }
    }

    entity::external_identity::Entity::delete_by_id(identity_id)
        .exec(conn)
        .await?;
    Ok(())
}

pub async fn delete_for_user<C: ConnectionTrait>(conn: &C, user_id: i32) -> CoreResult<()> {
    entity::external_identity::Entity::delete_many()
        .filter(entity::external_identity::Column::UserId.eq(user_id))
        .exec(conn)
        .await?;
    Ok(())
}
