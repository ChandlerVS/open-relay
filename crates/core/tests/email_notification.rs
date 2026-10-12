//! Round trip for per-form submission notifications.
//!
//! Pins the pieces that fail quietly:
//!
//! 1. `notification_email` is form metadata, validated and trimmed on write,
//!    and a blank value clears it rather than storing `""`.
//! 2. `create_submission` queues exactly one `email-notification` delivery,
//!    only when a recipient is set and only for a non-duplicate.
//! 3. `EmailNotificationBackend` fails *permanently* with no provider, sends
//!    through whatever provider is active, and no-ops once the recipient is
//!    cleared.
//!
//! The provider is a fake registered in this test, so nothing is ever sent to
//! AWS. The email provider is a deployment-wide singleton, so the test stashes
//! any row a dev database already has and puts it back at the end.
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test email_notification -- --ignored
//! ```

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use open_relay_core::backend::email_notification::{self, EmailNotificationBackend};
use open_relay_core::backend::{Backend, BackendRegistry, DeliveryError, DeliveryPayload};
use open_relay_core::crypto::SecretCipher;
use open_relay_core::email::{
    EmailBuildError, EmailError, EmailRegistry, EmailSender, EmailSenderFactory, OutgoingEmail,
};
use open_relay_core::email_config::{UpsertEmailConfig, service as email_config};
use open_relay_core::error::CoreError;
use open_relay_core::forms::{BackendBinding, NewForm, UpdateForm, service as forms_service};
use open_relay_core::metadata::{MetadataEntry, MetadataKey, MetadataValue, service as metadata};
use open_relay_core::submissions::{NewSubmissionPayload, service as submissions};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, Database, DatabaseConnection, EntityTrait,
    QueryFilter,
};
use serde_json::{Value as JsonValue, json};

type Outbox = Arc<Mutex<Vec<OutgoingEmail>>>;

/// Records what it was asked to send; `config.fail` picks a failure mode.
struct FakeFactory {
    outbox: Outbox,
}

struct FakeSender {
    outbox: Outbox,
    fail: Option<String>,
}

impl EmailSenderFactory for FakeFactory {
    fn kind(&self) -> &'static str {
        "fake"
    }
    fn label(&self) -> &'static str {
        "Fake"
    }
    fn secret_keys(&self) -> &'static [&'static str] {
        &["token"]
    }
    fn build(&self, config: &JsonValue) -> Result<Arc<dyn EmailSender>, EmailBuildError> {
        if config
            .get("token")
            .and_then(JsonValue::as_str)
            .unwrap_or("")
            .is_empty()
        {
            return Err(EmailBuildError::Invalid("token is required".into()));
        }
        Ok(Arc::new(FakeSender {
            outbox: self.outbox.clone(),
            fail: config
                .get("fail")
                .and_then(JsonValue::as_str)
                .map(str::to_string),
        }))
    }
}

#[async_trait]
impl EmailSender for FakeSender {
    fn kind(&self) -> &'static str {
        "fake"
    }
    async fn send(&self, email: &OutgoingEmail) -> Result<(), EmailError> {
        match self.fail.as_deref() {
            Some("permanent") => Err(EmailError::Permanent("sender not verified".into())),
            Some("transient") => Err(EmailError::Transient("throttled".into())),
            _ => {
                self.outbox.lock().unwrap().push(email.clone());
                Ok(())
            }
        }
    }
}

fn cipher() -> Arc<SecretCipher> {
    Arc::new(SecretCipher::from_key_bytes(&[7u8; 32]).unwrap())
}

fn no_uploads() -> submissions::UploadContext<'static> {
    static CIPHER: std::sync::OnceLock<SecretCipher> = std::sync::OnceLock::new();
    static REGISTRY: std::sync::OnceLock<open_relay_core::storage::StorageRegistry> =
        std::sync::OnceLock::new();
    submissions::UploadContext {
        cipher: CIPHER.get_or_init(|| SecretCipher::from_key_bytes(&[1u8; 32]).unwrap()),
        registry: REGISTRY.get_or_init(open_relay_core::storage::StorageRegistry::new),
    }
}

fn backends() -> BackendRegistry {
    let mut r = BackendRegistry::new();
    r.register_static(Arc::new(
        open_relay_core::backend::openrelay::OpenRelayBackend,
    ));
    r
}

fn new_form(slug: &str, metadata: Vec<MetadataEntry>) -> NewForm {
    NewForm {
        name: "Email notification integration".into(),
        display_name: Some("Contact us".into()),
        slug: Some(slug.into()),
        standard_fields: None,
        custom_fields: vec![],
        layout: None,
        backends: None,
        tags: vec![],
        reps: vec![],
        source_params: vec![],
        post_submission_action: Default::default(),
        progress_indicator: Default::default(),
        theme_id: None,
        metadata: Some(metadata),
    }
}

fn notify(value: &str) -> MetadataEntry {
    MetadataEntry {
        key: MetadataKey::NotificationEmail,
        value: MetadataValue::Text(value.into()),
    }
}

fn payload(email: &str) -> NewSubmissionPayload {
    NewSubmissionPayload(
        [
            ("first_name", json!("Jane")),
            ("last_name", json!("Doe")),
            ("email", json!(email)),
            ("message", json!("Hello\nthere")),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect(),
    )
}

async fn delivery_kinds(db: &DatabaseConnection, submission_id: i32) -> Vec<String> {
    let mut kinds: Vec<String> = entity::submission_delivery::Entity::find()
        .filter(entity::submission_delivery::Column::SubmissionId.eq(submission_id))
        .all(db)
        .await
        .expect("list deliveries")
        .into_iter()
        .map(|d| d.backend_name)
        .collect();
    kinds.sort();
    kinds
}

fn delivery(submission_id: i32, form_id: i32) -> DeliveryPayload {
    DeliveryPayload {
        submission_id,
        form_id,
        data: json!({ "first_name": "Jane", "email": "jane@acme.test" }),
        tags: vec!["expo".into()],
        assigned_to: None,
    }
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn notification_email_queues_sends_and_clears() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    // The `email_provider` table is new; make sure it exists the way server
    // boot would (idempotent, additive).
    db.get_schema_registry("entity::*")
        .sync(&db)
        .await
        .expect("schema sync");
    let reg = backends();
    let slug = format!("email-notification-it-{}", std::process::id());

    // --- 1. Metadata: validated, trimmed, cleared by blank. ---------------
    let bad = forms_service::create_form(
        &db,
        &reg,
        1,
        new_form(&format!("{slug}-bad"), vec![notify("a@x.com, b@x.com")]),
    )
    .await;
    assert!(matches!(bad, Err(CoreError::BadRequest(_))), "{bad:?}");

    let form = forms_service::create_form(
        &db,
        &reg,
        1,
        new_form(
            &slug,
            vec![
                notify("  ops@example.com "),
                MetadataEntry {
                    key: MetadataKey::EmailDeduplication,
                    value: MetadataValue::Bool(true),
                },
            ],
        ),
    )
    .await
    .expect("create form");
    assert_eq!(
        metadata::get_text(&db, form.id, MetadataKey::NotificationEmail)
            .await
            .unwrap()
            .as_deref(),
        Some("ops@example.com")
    );

    // The notification is never a bindable backend, even if registered.
    let mut with_static = backends();
    with_static.register_static(Arc::new(EmailNotificationBackend::new(
        db.clone(),
        EmailRegistry::new(),
        cipher(),
        "",
    )));
    let bound = forms_service::update_form(
        &db,
        &with_static,
        form.id,
        UpdateForm {
            backends: Some(vec![
                BackendBinding::open_relay(),
                BackendBinding {
                    kind: email_notification::NAME.into(),
                    instance_id: None,
                },
            ]),
            ..Default::default()
        },
    )
    .await;
    assert!(matches!(bound, Err(CoreError::BadRequest(_))), "{bound:?}");

    // --- 2. Queued once per non-duplicate submission. ----------------------
    let first =
        submissions::create_submission(&db, &form, payload("jane@acme.test"), &no_uploads())
            .await
            .expect("first submission");
    assert_eq!(
        delivery_kinds(&db, first.id).await,
        vec!["email-notification".to_string(), "open-relay".to_string()]
    );
    let dup = submissions::create_submission(&db, &form, payload("jane@acme.test"), &no_uploads())
        .await
        .expect("duplicate submission");
    assert_eq!(dup.is_duplicate, Some(true));
    assert!(
        delivery_kinds(&db, dup.id).await.is_empty(),
        "a duplicate never notifies"
    );

    // --- 3. The backend. ----------------------------------------------------
    let outbox: Outbox = Arc::default();
    let mut email = EmailRegistry::new();
    email.register_factory(Arc::new(FakeFactory {
        outbox: outbox.clone(),
    }));
    let cipher = cipher();
    let backend = EmailNotificationBackend::new(
        db.clone(),
        email.clone(),
        cipher.clone(),
        "https://admin.example.com/",
    );

    // Stash a dev database's real provider so the singleton is ours.
    let stashed = email_config::get_active(&db).await.expect("read provider");
    if stashed.is_some() {
        email_config::delete_active(&db)
            .await
            .expect("stash provider");
    }

    // Run in a task so a failed assertion still reaches the restore below.
    let (task_db, task_form, task_first) = (db.clone(), form.clone(), first.clone());
    let outcome = tokio::spawn(async move {
        let (db, form, first) = (task_db, task_form, task_first);
        let reg = backends();
        // No provider: permanent, with a message pointing at the fix.
        match backend.deliver(&delivery(first.id, form.id)).await {
            Err(DeliveryError::Permanent(msg)) => {
                assert!(msg.contains("no email provider configured"), "{msg}")
            }
            other => panic!("expected a permanent failure, got {other:?}"),
        }

        // Provider configured: the secret is sealed at rest and redacted.
        let saved = email_config::upsert(
            &db,
            &email,
            &cipher,
            UpsertEmailConfig {
                kind: "fake".into(),
                name: "Fake".into(),
                config: json!({ "token": "s3cret" }),
            },
        )
        .await
        .expect("save provider");
        assert!(saved.config["token"].as_str().unwrap().starts_with("enc:"));
        let dto = email_config::to_dto(&email, saved);
        assert!(dto.config.get("token").is_none());
        assert_eq!(dto.secret_fields.get("token"), Some(&true));

        backend
            .deliver(&delivery(first.id, form.id))
            .await
            .expect("send");
        {
            let sent = outbox.lock().unwrap();
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].to, "ops@example.com");
            assert_eq!(sent[0].subject, "New submission: Contact us");
            assert!(
                sent[0].text.contains("First name: Jane"),
                "{}",
                sent[0].text
            );
            assert!(sent[0].text.contains("Tags: expo"), "{}", sent[0].text);
            assert!(
                sent[0].text.contains(&format!(
                    "https://admin.example.com/submissions?submission={}",
                    first.id
                )),
                "{}",
                sent[0].text
            );
        }

        // A partial update keeps the secret; a rejection is permanent.
        email_config::upsert(
            &db,
            &email,
            &cipher,
            UpsertEmailConfig {
                kind: "fake".into(),
                name: "Fake".into(),
                config: json!({ "fail": "permanent" }),
            },
        )
        .await
        .expect("partial update keeps the token");
        assert!(matches!(
            backend.deliver(&delivery(first.id, form.id)).await,
            Err(DeliveryError::Permanent(_))
        ));

        // Cleared recipient: blank deletes the row, and the queued delivery
        // becomes a successful no-op.
        forms_service::update_form(
            &db,
            &reg,
            form.id,
            UpdateForm {
                metadata: Some(vec![notify("   ")]),
                ..Default::default()
            },
        )
        .await
        .expect("clear recipient");
        assert_eq!(
            metadata::get(&db, form.id, MetadataKey::NotificationEmail)
                .await
                .unwrap(),
            None
        );
        backend
            .deliver(&delivery(first.id, form.id))
            .await
            .expect("no-op once cleared");
        assert_eq!(outbox.lock().unwrap().len(), 1);

        // And nothing is queued for the next submission.
        let later =
            submissions::create_submission(&db, &form, payload("john@acme.test"), &no_uploads())
                .await
                .expect("later submission");
        assert_eq!(
            delivery_kinds(&db, later.id).await,
            vec!["open-relay".to_string()]
        );
    })
    .await;

    // Put the dev database back the way it was.
    let _ = email_config::delete_active(&db).await;
    if let Some(prev) = stashed {
        entity::email_provider::ActiveModel {
            kind: ActiveValue::Set(prev.kind),
            name: ActiveValue::Set(prev.name),
            is_active: ActiveValue::Set(prev.is_active),
            config: ActiveValue::Set(prev.config),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("restore provider");
    }
    let _ = forms_service::delete_form(&db, form.id).await;
    if let Err(e) = outcome {
        std::panic::resume_unwind(e.into_panic());
    }
}
