//! Persistence + validation for configurable backend instances.

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder,
};

use super::{
    BackendInstanceDto, BackendInstanceFormRef, BackendInstanceInUse, BackendInstanceList,
    NewBackendInstance, UpdateBackendInstance, value_is_empty,
};
use crate::backend::{BackendBuildError, BackendRegistry};
use crate::crypto::SecretCipher;
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

pub async fn list<C: ConnectionTrait>(
    conn: &C,
    registry: &BackendRegistry,
) -> CoreResult<BackendInstanceList> {
    let rows = entity::backend_instance::Entity::find()
        .order_by_asc(entity::backend_instance::Column::Name)
        .all(conn)
        .await?;
    let total = rows.len() as u64;
    let items: Vec<BackendInstanceDto> = rows
        .into_iter()
        .map(|m| BackendInstanceDto::from_model(registry, m))
        .collect();
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
    cipher: &SecretCipher,
    input: NewBackendInstance,
) -> CoreResult<entity::backend_instance::Model> {
    let kind = input.kind.trim().to_string();
    if !registry.is_configurable(&kind) {
        return Err(CoreError::BadRequest(format!(
            "backend kind '{kind}' is not configurable"
        )));
    }
    let name = validate_name(&input.name)?;
    // Validate against the plaintext config (the factory rejects empty tokens),
    // then encrypt secret-bearing keys before they touch the DB.
    validate_config(registry, &kind, &input.config)?;
    let mut config = input.config;
    encrypt_secret_keys(registry, &kind, &mut config, cipher)?;

    let active = entity::backend_instance::ActiveModel {
        kind: ActiveValue::Set(kind),
        name: ActiveValue::Set(name),
        config: ActiveValue::Set(config),
        ..Default::default()
    };
    Ok(active.insert(conn).await?)
}

pub async fn update<C: ConnectionTrait>(
    conn: &C,
    registry: &BackendRegistry,
    cipher: &SecretCipher,
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

    if let Some(mut config) = input.config {
        // Secrets are redacted out of the DTO the admin GET'd, so a round-tripped
        // config omits them. Treat a missing/empty secret key as "keep existing"
        // (mirrors OAuth `client_secret: None`) rather than clobbering the token.
        // `existing.config` holds *encrypted* secret blobs, so the carried-over
        // values are ciphertext at this point.
        preserve_secrets(registry, &existing.kind, &existing.config, &mut config);
        // Bring every secret key to a uniform plaintext view (newly-supplied
        // values pass through untouched; carried-over ciphertext is decrypted),
        // validate against plaintext, then re-encrypt before persisting.
        decrypt_secret_keys(registry, &existing.kind, &mut config, cipher)?;
        validate_config(registry, &existing.kind, &config)?;
        encrypt_secret_keys(registry, &existing.kind, &mut config, cipher)?;
        active.config = ActiveValue::Set(config);
    }

    Ok(active.update(conn).await?)
}

/// For each secret key the kind declares, if `incoming` omits it (absent, null,
/// or empty string) but `existing` has a value, carry the existing value over.
fn preserve_secrets(
    registry: &BackendRegistry,
    kind: &str,
    existing: &serde_json::Value,
    incoming: &mut serde_json::Value,
) {
    let Some(incoming_obj) = incoming.as_object_mut() else {
        return;
    };
    for key in registry.secret_keys(kind) {
        let incoming_empty = incoming_obj.get(*key).map(value_is_empty).unwrap_or(true);
        if !incoming_empty {
            continue;
        }
        if let Some(existing_val) = existing.get(*key).filter(|v| !value_is_empty(v)) {
            incoming_obj.insert((*key).to_string(), existing_val.clone());
        }
    }
}

/// Encrypt each declared secret key in `config` that holds a non-empty string
/// and isn't already ciphertext. Idempotent — safe to call on a config whose
/// secrets are already encrypted.
pub(crate) fn encrypt_secret_keys(
    registry: &BackendRegistry,
    kind: &str,
    config: &mut serde_json::Value,
    cipher: &SecretCipher,
) -> CoreResult<()> {
    let Some(obj) = config.as_object_mut() else {
        return Ok(());
    };
    for key in registry.secret_keys(kind) {
        if let Some(serde_json::Value::String(s)) = obj.get(*key) {
            if !s.trim().is_empty() && !SecretCipher::is_encrypted(s) {
                let enc = cipher.encrypt(s)?;
                obj.insert((*key).to_string(), serde_json::Value::String(enc));
            }
        }
    }
    Ok(())
}

/// Decrypt each declared secret key in `config` back to plaintext. A legacy
/// plaintext value (no `enc:v1:` prefix) passes through unchanged.
pub(crate) fn decrypt_secret_keys(
    registry: &BackendRegistry,
    kind: &str,
    config: &mut serde_json::Value,
    cipher: &SecretCipher,
) -> CoreResult<()> {
    let Some(obj) = config.as_object_mut() else {
        return Ok(());
    };
    for key in registry.secret_keys(kind) {
        if let Some(serde_json::Value::String(s)) = obj.get(*key) {
            let plain = cipher.decrypt(s)?;
            obj.insert((*key).to_string(), serde_json::Value::String(plain));
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::GoHighLevelFactory;
    use std::sync::Arc;

    fn registry() -> BackendRegistry {
        let mut r = BackendRegistry::new();
        r.register_factory(Arc::new(GoHighLevelFactory::new()));
        r
    }

    #[test]
    fn preserve_secrets_keeps_existing_when_omitted() {
        let existing = serde_json::json!({
            "location_id": "loc_old",
            "private_integration_token": "pit-existing",
        });
        // Admin round-trips the redacted config (no token) with a changed location.
        let mut incoming = serde_json::json!({ "location_id": "loc_new" });
        preserve_secrets(&registry(), "gohighlevel", &existing, &mut incoming);
        assert_eq!(incoming["location_id"], "loc_new");
        assert_eq!(incoming["private_integration_token"], "pit-existing");
    }

    #[test]
    fn preserve_secrets_respects_explicit_new_value() {
        let existing = serde_json::json!({ "private_integration_token": "pit-old" });
        let mut incoming = serde_json::json!({ "private_integration_token": "pit-new" });
        preserve_secrets(&registry(), "gohighlevel", &existing, &mut incoming);
        assert_eq!(incoming["private_integration_token"], "pit-new");
    }

    #[test]
    fn preserve_secrets_treats_empty_string_as_omitted() {
        let existing = serde_json::json!({ "private_integration_token": "pit-old" });
        let mut incoming = serde_json::json!({ "private_integration_token": "" });
        preserve_secrets(&registry(), "gohighlevel", &existing, &mut incoming);
        assert_eq!(incoming["private_integration_token"], "pit-old");
    }

    fn cipher() -> SecretCipher {
        SecretCipher::from_key_bytes(&[3u8; crate::crypto::KEY_LEN]).unwrap()
    }

    #[test]
    fn encrypt_then_decrypt_secret_keys_round_trips() {
        let c = cipher();
        let mut config = serde_json::json!({
            "location_id": "loc_1",
            "private_integration_token": "pit-live-xyz",
        });
        encrypt_secret_keys(&registry(), "gohighlevel", &mut config, &c).unwrap();
        // Secret is now ciphertext; non-secret untouched.
        let stored = config["private_integration_token"].as_str().unwrap();
        assert!(SecretCipher::is_encrypted(stored));
        assert_eq!(config["location_id"], "loc_1");

        decrypt_secret_keys(&registry(), "gohighlevel", &mut config, &c).unwrap();
        assert_eq!(config["private_integration_token"], "pit-live-xyz");
        assert_eq!(config["location_id"], "loc_1");
    }

    #[test]
    fn encrypt_secret_keys_is_idempotent() {
        let c = cipher();
        let mut config = serde_json::json!({ "private_integration_token": "pit" });
        encrypt_secret_keys(&registry(), "gohighlevel", &mut config, &c).unwrap();
        let once = config["private_integration_token"].as_str().unwrap().to_string();
        encrypt_secret_keys(&registry(), "gohighlevel", &mut config, &c).unwrap();
        // Already-encrypted value is left as-is (not double-encrypted).
        assert_eq!(config["private_integration_token"], once);
    }
}
