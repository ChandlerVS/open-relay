//! Email notification backend: tells a person a submission arrived.
//!
//! Not a destination an admin binds to a form. It is queued by
//! `submissions::service::create_submission` whenever the form carries a
//! `notification_email` metadata value, and rides the delivery worker for its
//! retry/backoff, stale-lease recovery and per-submission status — so a missing
//! or misconfigured email provider shows up as a failed delivery in the admin,
//! not as a silent drop.
//!
//! Three things worth knowing:
//!
//! 1. **Delivery is at-least-once.** No email provider takes an idempotency
//!    key, so a worker that crashes after SES accepted the message (or a
//!    response lost in transit, retried as transient) sends it again. The
//!    `Backend::deliver` idempotency contract is met only in the weak sense
//!    that a duplicate notification is harmless.
//! 2. **The recipient is re-read at delivery time.** Clearing a form's address
//!    stops notifications already queued — the delivery succeeds as a no-op —
//!    and changing it redirects them.
//! 3. **The body is plain text and visitor data never reaches a header.** The
//!    subject names the form (admin-chosen); every answer goes in the body.

use std::sync::Arc;

use async_trait::async_trait;
use sea_orm::{DatabaseConnection, EntityTrait};
use super::summary::render_answers;
use super::{Backend, DeliveryError, DeliveryPayload};
use crate::crypto::SecretCipher;
use crate::email::{EmailError, EmailRegistry, OutgoingEmail};
use crate::email_config::service as email_config_service;
use crate::error::CoreError;
use crate::forms::{CustomField, service as forms_service};
use crate::metadata::{MetadataKey, service as metadata_service};

pub const NAME: &str = "email-notification";

pub struct EmailNotificationBackend {
    db: DatabaseConnection,
    email: EmailRegistry,
    cipher: Arc<SecretCipher>,
    /// Admin SPA origin, no trailing slash — for the "view submission" link.
    admin_url: String,
}

impl EmailNotificationBackend {
    pub fn new(
        db: DatabaseConnection,
        email: EmailRegistry,
        cipher: Arc<SecretCipher>,
        admin_url: impl Into<String>,
    ) -> Self {
        Self {
            db,
            email,
            cipher,
            admin_url: admin_url.into().trim_end_matches('/').to_string(),
        }
    }
}

/// A DB hiccup is worth retrying; anything else (a stored config the build
/// can no longer load, a kind that was removed) needs an admin.
fn from_core(e: CoreError) -> DeliveryError {
    match e {
        CoreError::Db(db) => DeliveryError::Transient(format!("database error: {db}")),
        other => DeliveryError::Permanent(format!("email provider unusable: {other:?}")),
    }
}

#[async_trait]
impl Backend for EmailNotificationBackend {
    fn name(&self) -> &'static str {
        NAME
    }

    async fn deliver(&self, payload: &DeliveryPayload) -> Result<(), DeliveryError> {
        let Some(to) =
            metadata_service::get_text(&self.db, payload.form_id, MetadataKey::NotificationEmail)
                .await
                .map_err(from_core)?
        else {
            // Cleared since the submission was queued: nothing to do.
            return Ok(());
        };

        let sender = email_config_service::load_active_sender(&self.db, &self.email, &self.cipher)
            .await
            .map_err(from_core)?
            .ok_or_else(|| {
                DeliveryError::Permanent(
                    "no email provider configured — set one up under Settings → Email".into(),
                )
            })?;

        let form = entity::form::Entity::find_by_id(payload.form_id)
            .one(&self.db)
            .await
            .map_err(|e| DeliveryError::Transient(format!("database error: {e}")))?;
        let (form_name, custom_fields) = match &form {
            Some(f) => (
                f.display_name.clone().unwrap_or_else(|| f.name.clone()),
                // Labels are a nicety: a form whose layout no longer parses
                // still gets its notification, keyed by field key.
                forms_service::layout_from_model(f)
                    .map(|l| forms_service::legacy_from_layout(&l).1)
                    .unwrap_or_default(),
            ),
            None => (format!("form #{}", payload.form_id), Vec::new()),
        };

        let email = OutgoingEmail {
            to,
            subject: format!("New submission: {form_name}"),
            text: render_body(&form_name, payload, &custom_fields, &self.admin_url),
        };
        sender.send(&email).await.map_err(|e| match e {
            EmailError::Transient(m) => DeliveryError::Transient(m),
            EmailError::Permanent(m) | EmailError::Invalid(m) => DeliveryError::Permanent(m),
        })
    }
}

/// The plain-text notification body: a header naming the form, the answers
/// (see [`render_answers`] for the order), then tags and a link to the admin.
fn render_body(
    form_name: &str,
    payload: &DeliveryPayload,
    custom_fields: &[CustomField],
    admin_url: &str,
) -> String {
    let mut out = format!(
        "A new submission (#{}) arrived on \"{form_name}\".\n\n",
        payload.submission_id
    );

    let labels: Vec<(String, String)> = custom_fields
        .iter()
        .map(|f| (f.key.clone(), f.label.clone()))
        .collect();
    out.push_str(&render_answers(&payload.data, &labels));

    if !payload.tags.is_empty() {
        out.push_str(&format!("\nTags: {}\n", payload.tags.join(", ")));
    }
    if !admin_url.is_empty() {
        out.push_str(&format!(
            "\nView it in OpenRelay: {admin_url}/submissions?submission={}\n",
            payload.submission_id
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn payload(data: Value, tags: Vec<String>) -> DeliveryPayload {
        DeliveryPayload {
            submission_id: 42,
            form_id: 7,
            form_name: "Contact us".into(),
            form_slug: "contact-us".into(),
            field_labels: Vec::new(),
            data,
            tags,
            assigned_to: None,
        }
    }

    fn custom(key: &str, label: &str) -> CustomField {
        serde_json::from_value(json!({
            "key": key, "label": label, "type": "text", "position": 0,
        }))
        .unwrap()
    }

    #[test]
    fn body_orders_standard_then_custom_then_leftovers() {
        let p = payload(
            json!({
                "zzz_removed": "old",
                "budget": "10k",
                "email": "jane@acme.test",
                "first_name": "Jane",
                "interests": ["A", "C"],
                "consent": true,
                "empty": "",
                "nothing": null,
            }),
            vec!["expo".into(), "rep:jane".into()],
        );
        let body = render_body(
            "Contact us",
            &p,
            &[custom("budget", "Budget"), custom("interests", "")],
            "https://admin.example.com",
        );
        assert_eq!(
            body,
            "A new submission (#42) arrived on \"Contact us\".\n\n\
First name: Jane\n\
Email: jane@acme.test\n\
Budget: 10k\n\
interests: A, C\n\
consent: Yes\n\
zzz_removed: old\n\
\nTags: expo, rep:jane\n\
\nView it in OpenRelay: https://admin.example.com/submissions?submission=42\n"
        );
    }
}
