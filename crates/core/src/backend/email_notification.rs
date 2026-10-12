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

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use sea_orm::{DatabaseConnection, EntityTrait};
use serde_json::Value;

use super::{Backend, DeliveryError, DeliveryPayload};
use crate::crypto::SecretCipher;
use crate::email::{EmailError, EmailRegistry, OutgoingEmail};
use crate::email_config::service as email_config_service;
use crate::error::CoreError;
use crate::forms::{CustomField, STANDARD_FIELD_KEYS, service as forms_service};
use crate::metadata::{MetadataKey, service as metadata_service};

pub const NAME: &str = "email-notification";

/// Cap on one rendered answer, so a 10 000-character message field can't
/// turn a notification into a wall of text. The full value is in the admin.
const MAX_VALUE_CHARS: usize = 2_000;

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

/// The plain-text notification body: standard fields in catalogue order, then
/// custom fields in layout order under their labels, then anything else the
/// payload carries (a field since removed from the form), sorted by key.
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

    let data = payload.data.as_object();
    let mut seen: HashSet<String> = HashSet::new();
    let mut lines: Vec<(String, String)> = Vec::new();
    let mut push = |label: String, key: &str| {
        let Some(value) = data.and_then(|d| d.get(key)) else {
            return;
        };
        if !seen.insert(key.to_string()) {
            return;
        }
        if let Some(text) = render_value(value) {
            lines.push((label, text));
        }
    };
    for key in STANDARD_FIELD_KEYS {
        push(humanize(key), key);
    }
    for field in custom_fields {
        let label = match field.label.trim() {
            "" => field.key.clone(),
            l => l.to_string(),
        };
        push(label, &field.key);
    }
    if let Some(d) = data {
        let mut rest: Vec<&String> = d.keys().collect();
        rest.sort();
        for key in rest {
            push(key.clone(), key);
        }
    }

    if lines.is_empty() {
        out.push_str("(no answers)\n");
    }
    for (label, value) in lines {
        if value.contains('\n') {
            // Indent continuation lines so a multi-line answer can't pass
            // itself off as another field.
            out.push_str(&format!("{label}:\n"));
            for line in value.lines() {
                out.push_str(&format!("    {line}\n"));
            }
        } else {
            out.push_str(&format!("{label}: {value}\n"));
        }
    }

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

/// One answer as text; `None` for an empty one.
fn render_value(value: &Value) -> Option<String> {
    let text = match value {
        Value::Null => return None,
        Value::String(s) => s.clone(),
        Value::Bool(b) => if *b { "Yes" } else { "No" }.to_string(),
        Value::Number(n) => n.to_string(),
        // Checkbox groups: the ticked options.
        Value::Array(items) => items
            .iter()
            .filter_map(render_value)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(_) => value.to_string(),
    };
    // Control characters other than newline/tab have no business in a
    // plain-text body (a bare CR could overwrite a line in some clients).
    let text: String = text
        .replace("\r\n", "\n")
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(if text.chars().count() > MAX_VALUE_CHARS {
        let mut cut: String = text.chars().take(MAX_VALUE_CHARS).collect();
        cut.push('…');
        cut
    } else {
        text.to_string()
    })
}

/// `address_line_1` → `Address line 1`.
fn humanize(key: &str) -> String {
    let spaced = key.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload(data: Value, tags: Vec<String>) -> DeliveryPayload {
        DeliveryPayload {
            submission_id: 42,
            form_id: 7,
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

    #[test]
    fn multi_line_answers_are_indented_and_stripped_of_control_chars() {
        let p = payload(json!({ "message": "line one\r\nline two\u{7}" }), vec![]);
        let body = render_body("F", &p, &[], "");
        assert!(
            body.contains("Message:\n    line one\n    line two\n"),
            "{body}"
        );
        assert!(!body.contains('\r'));
        assert!(!body.contains("View it"));
    }

    #[test]
    fn long_answers_are_truncated() {
        let p = payload(
            json!({ "message": "x".repeat(MAX_VALUE_CHARS + 50) }),
            vec![],
        );
        let body = render_body("F", &p, &[], "");
        assert!(body.contains('…'));
        assert!(!body.contains(&"x".repeat(MAX_VALUE_CHARS + 1)));
    }

    #[test]
    fn empty_submission_says_so() {
        let body = render_body("F", &payload(json!({}), vec![]), &[], "");
        assert!(body.contains("(no answers)"));
    }

    #[test]
    fn humanize_capitalises_first_word_only() {
        assert_eq!(humanize("address_line_1"), "Address line 1");
        assert_eq!(humanize("email"), "Email");
    }
}
