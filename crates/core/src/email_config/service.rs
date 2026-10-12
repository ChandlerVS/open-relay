//! Persistence + validation for the (singleton) active email provider.
//!
//! The update ordering — preserve, decrypt, validate, encrypt — is the same
//! one `storage_config::service` uses and is documented in [`crate::secrets`].

use std::sync::Arc;

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
};

use super::{EmailConfigDto, EmailTestResult, UpsertEmailConfig};
use crate::crypto::SecretCipher;
use crate::email::{EmailBuildError, EmailRegistry, EmailSender, OutgoingEmail};
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

pub async fn get_active<C: ConnectionTrait>(
    conn: &C,
) -> CoreResult<Option<entity::email_provider::Model>> {
    Ok(entity::email_provider::Entity::find()
        .filter(entity::email_provider::Column::IsActive.eq(true))
        .one(conn)
        .await?)
}

/// Build the active sender. `Ok(None)` means no provider is configured — a
/// distinct, non-error state the notification backend reports on its own
/// terms.
pub async fn load_active_sender<C: ConnectionTrait>(
    conn: &C,
    registry: &EmailRegistry,
    cipher: &SecretCipher,
) -> CoreResult<Option<Arc<dyn EmailSender>>> {
    let Some(model) = get_active(conn).await? else {
        return Ok(None);
    };
    Ok(Some(build_sender(registry, cipher, &model)?))
}

/// Decrypt a row's secrets and hand it to its factory.
fn build_sender(
    registry: &EmailRegistry,
    cipher: &SecretCipher,
    model: &entity::email_provider::Model,
) -> CoreResult<Arc<dyn EmailSender>> {
    let factory = registry.get_factory(&model.kind).ok_or_else(|| {
        CoreError::Internal(anyhow::anyhow!(
            "email kind '{}' is not registered in this build",
            model.kind
        ))
    })?;
    let mut config = model.config.clone();
    crate::secrets::decrypt_in_place(factory.secret_keys(), &mut config, cipher)?;
    factory
        .build(&config)
        .map_err(|EmailBuildError::Invalid(msg)| {
            CoreError::Internal(anyhow::anyhow!("stored email config is unusable: {msg}"))
        })
}

/// Create or replace the active provider, updating the row in place so
/// `created_at` keeps meaning "when email was first configured".
pub async fn upsert<C: ConnectionTrait>(
    conn: &C,
    registry: &EmailRegistry,
    cipher: &SecretCipher,
    input: UpsertEmailConfig,
) -> CoreResult<entity::email_provider::Model> {
    let kind = input.kind.trim().to_string();
    if !registry.knows(&kind) {
        return Err(CoreError::BadRequest(format!(
            "email kind '{kind}' is not supported"
        )));
    }
    let name = validate_name(&input.name)?;
    let secret_keys = registry.secret_keys(&kind);
    let mut config = input.config;

    let existing = get_active(conn).await?;

    // Carry over secrets the client omitted — only when the kind is unchanged,
    // since one provider's secret is meaningless to another.
    if let Some(prev) = &existing
        && prev.kind == kind
    {
        crate::secrets::preserve(secret_keys, &prev.config, &mut config);
    }

    crate::secrets::decrypt_in_place(secret_keys, &mut config, cipher)?;
    validate_config(registry, &kind, &config)?;
    crate::secrets::encrypt_in_place(secret_keys, &mut config, cipher)?;

    match existing {
        Some(prev) => {
            let mut active: entity::email_provider::ActiveModel = prev.into();
            active.kind = ActiveValue::Set(kind);
            active.name = ActiveValue::Set(name);
            active.config = ActiveValue::Set(config);
            active.is_active = ActiveValue::Set(true);
            Ok(active.update(conn).await?)
        }
        None => {
            let active = entity::email_provider::ActiveModel {
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
    registry: &EmailRegistry,
    kind: &str,
    config: &serde_json::Value,
) -> CoreResult<()> {
    let factory = registry
        .get_factory(kind)
        .ok_or_else(|| CoreError::BadRequest(format!("email kind '{kind}' is not supported")))?;
    factory
        .build(config)
        .map(|_| ())
        .map_err(|EmailBuildError::Invalid(msg)| CoreError::BadRequest(msg))
}

pub async fn delete_active<C: ConnectionTrait>(conn: &C) -> CoreResult<()> {
    let Some(model) = get_active(conn).await? else {
        return Err(CoreError::NotFound("no email provider configured".into()));
    };
    entity::email_provider::Entity::delete_by_id(model.id)
        .exec(conn)
        .await?;
    Ok(())
}

/// Send a real message to `to` through the saved provider.
///
/// A provider that answers with a rejection is reported as
/// `Ok(EmailTestResult { ok: false, .. })` rather than an `Err`: the message
/// *is* the answer the admin asked for (typically "the recipient isn't
/// verified in the SES sandbox").
pub async fn send_test<C: ConnectionTrait>(
    conn: &C,
    registry: &EmailRegistry,
    cipher: &SecretCipher,
    to: &str,
) -> CoreResult<EmailTestResult> {
    let to = crate::email::validate_address(to).map_err(CoreError::BadRequest)?;
    let sender = load_active_sender(conn, registry, cipher)
        .await?
        .ok_or_else(|| CoreError::NotFound("no email provider configured".into()))?;
    let email = OutgoingEmail {
        to,
        subject: "OpenRelay test email".into(),
        text: "This is a test message from OpenRelay.\n\n\
               If you can read it, submission notifications will be delivered \
               through this provider.\n"
            .into(),
    };
    Ok(match sender.send(&email).await {
        Ok(()) => EmailTestResult {
            ok: true,
            error: None,
        },
        Err(e) => EmailTestResult {
            ok: false,
            error: Some(e.message().to_string()),
        },
    })
}

/// Redacted DTO for a row, for handlers that already hold the model.
pub fn to_dto(registry: &EmailRegistry, model: entity::email_provider::Model) -> EmailConfigDto {
    EmailConfigDto::from_model(registry, model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_trims_and_bounds() {
        assert_eq!(validate_name("  Mail  ").unwrap(), "Mail");
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"x".repeat(MAX_NAME_LEN + 1)).is_err());
    }

    #[test]
    fn validate_config_rejects_unknown_kind() {
        let registry = EmailRegistry::new();
        assert!(validate_config(&registry, "ses", &serde_json::json!({})).is_err());
    }
}
