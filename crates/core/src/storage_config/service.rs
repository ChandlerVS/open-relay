//! Persistence + validation for the (singleton) active storage provider.
//!
//! The update ordering — preserve, decrypt, validate, encrypt — is the same
//! one `backends::service` uses and is documented in [`crate::secrets`].

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};

use super::{StorageConfigDto, UpsertStorageConfig};
use crate::crypto::SecretCipher;
use crate::error::{CoreError, CoreResult};
use crate::storage::{FileStore, StorageBuildError, StorageError, StorageRegistry};

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

pub async fn get_active<C: ConnectionTrait>(
    conn: &C,
) -> CoreResult<Option<entity::storage_provider::Model>> {
    Ok(entity::storage_provider::Entity::find()
        .filter(entity::storage_provider::Column::IsActive.eq(true))
        .one(conn)
        .await?)
}

/// Whether *any* storage provider is configured. Cheap presence check for the
/// public form DTO, which only needs to tell the renderer whether an upload
/// can succeed at all.
pub async fn is_configured<C: ConnectionTrait>(conn: &C) -> CoreResult<bool> {
    Ok(get_active(conn).await?.is_some())
}

/// Build the active store, ready to presign. `Ok(None)` means no provider is
/// configured — a distinct, non-error state that the callers handle explicitly
/// (a form with a file field is still perfectly saveable without one).
pub async fn load_active_store<C: ConnectionTrait>(
    conn: &C,
    registry: &StorageRegistry,
    cipher: &SecretCipher,
) -> CoreResult<Option<Arc<dyn FileStore>>> {
    let Some(model) = get_active(conn).await? else {
        return Ok(None);
    };
    Ok(Some(build_store(registry, cipher, &model)?))
}

/// Decrypt a row's secrets and hand it to its factory.
fn build_store(
    registry: &StorageRegistry,
    cipher: &SecretCipher,
    model: &entity::storage_provider::Model,
) -> CoreResult<Arc<dyn FileStore>> {
    let factory = registry.get_factory(&model.kind).ok_or_else(|| {
        // The row names a kind this build doesn't register — a downgrade, or a
        // provider that was removed. Not the submitter's fault, so it reads as
        // a server-side misconfiguration rather than a bad request.
        CoreError::Internal(anyhow::anyhow!(
            "storage kind '{}' is not registered in this build",
            model.kind
        ))
    })?;
    let mut config = model.config.clone();
    crate::secrets::decrypt_in_place(factory.secret_keys(), &mut config, cipher)?;
    factory.build(&config).map_err(|StorageBuildError::Invalid(msg)| {
        CoreError::Internal(anyhow::anyhow!("stored storage config is unusable: {msg}"))
    })
}

/// Create or replace the active provider.
///
/// The kind may change between calls (S3 today, another kind later); the row
/// is updated in place rather than replaced so `created_at` keeps meaning
/// "when storage was first configured".
pub async fn upsert<C: ConnectionTrait>(
    conn: &C,
    registry: &StorageRegistry,
    cipher: &SecretCipher,
    input: UpsertStorageConfig,
) -> CoreResult<entity::storage_provider::Model> {
    let kind = input.kind.trim().to_string();
    if !registry.knows(&kind) {
        return Err(CoreError::BadRequest(format!(
            "storage kind '{kind}' is not supported"
        )));
    }
    let name = validate_name(&input.name)?;
    let secret_keys = registry.secret_keys(&kind);
    let mut config = input.config;

    let existing = get_active(conn).await?;

    // Carry over secrets the client omitted — but only when the kind hasn't
    // changed. An S3 secret is meaningless to a different provider, and
    // silently reusing it would produce a config that validates against the
    // wrong credential.
    if let Some(prev) = &existing
        && prev.kind == kind
    {
        crate::secrets::preserve(secret_keys, &prev.config, &mut config);
    }

    // Uniform plaintext view, validate against it (the factory rejects an
    // empty credential, which is what makes "create with no secret" fail),
    // then re-seal before the row is written.
    crate::secrets::decrypt_in_place(secret_keys, &mut config, cipher)?;
    validate_config(registry, &kind, &config)?;
    crate::secrets::encrypt_in_place(secret_keys, &mut config, cipher)?;

    match existing {
        Some(prev) => {
            let mut active: entity::storage_provider::ActiveModel = prev.into();
            active.kind = ActiveValue::Set(kind);
            active.name = ActiveValue::Set(name);
            active.config = ActiveValue::Set(config);
            active.is_active = ActiveValue::Set(true);
            Ok(active.update(conn).await?)
        }
        None => {
            let active = entity::storage_provider::ActiveModel {
                kind: ActiveValue::Set(kind),
                name: ActiveValue::Set(name),
                is_active: ActiveValue::Set(true),
                config: ActiveValue::Set(config),
                ..Default::default()
            };
            Ok(active.insert(conn).await?)
        }
    }
}

/// Run the candidate config through its factory so a config the runtime can't
/// load never lands in the DB.
fn validate_config(
    registry: &StorageRegistry,
    kind: &str,
    config: &serde_json::Value,
) -> CoreResult<()> {
    let factory = registry
        .get_factory(kind)
        .ok_or_else(|| CoreError::BadRequest(format!("storage kind '{kind}' is not supported")))?;
    factory
        .build(config)
        .map(|_| ())
        .map_err(|StorageBuildError::Invalid(msg)| CoreError::BadRequest(msg))
}

pub async fn delete_active<C: ConnectionTrait>(conn: &C) -> CoreResult<()> {
    let Some(model) = get_active(conn).await? else {
        return Err(CoreError::NotFound("no storage provider configured".into()));
    };
    entity::storage_provider::Entity::delete_by_id(model.id)
        .exec(conn)
        .await?;
    Ok(())
}

/// Probe the stored credentials by round-tripping a real object.
///
/// Deliberately returns `Ok(StorageTestResult { ok: false, .. })` rather than
/// an `Err` for a reachable-but-rejecting store: this is a diagnostic the admin
/// asked for, and the failure message *is* the answer, not an exception.
pub async fn test_active<C: ConnectionTrait>(
    conn: &C,
    registry: &StorageRegistry,
    cipher: &SecretCipher,
) -> CoreResult<super::StorageTestResult> {
    let store = load_active_store(conn, registry, cipher)
        .await?
        .ok_or_else(|| CoreError::NotFound("no storage provider configured".into()))?;
    Ok(match store.check().await {
        Ok(()) => super::StorageTestResult {
            ok: true,
            error: None,
        },
        Err(StorageError::Unavailable(msg) | StorageError::Invalid(msg)) => {
            super::StorageTestResult {
                ok: false,
                error: Some(msg),
            }
        }
    })
}

/// Redacted DTO for a row, for handlers that already hold the model.
pub fn to_dto(
    registry: &StorageRegistry,
    model: entity::storage_provider::Model,
) -> StorageConfigDto {
    StorageConfigDto::from_model(registry, model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_trims_and_bounds() {
        assert_eq!(validate_name("  Uploads  ").unwrap(), "Uploads");
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"x".repeat(MAX_NAME_LEN + 1)).is_err());
    }

    #[test]
    fn validate_config_rejects_unknown_kind() {
        let registry = StorageRegistry::new();
        assert!(validate_config(&registry, "s3", &serde_json::json!({})).is_err());
    }
}
