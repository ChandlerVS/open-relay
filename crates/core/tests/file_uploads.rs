//! Read-after-write checks for `file` form fields, end to end.
//!
//! Four things about this feature only break in ways a live database can
//! reveal, so the unit tests over the pure functions aren't enough:
//!
//! 1. `CustomFieldType::File` and its `accept` / `max_size_mb` payload have to
//!    survive a real JSON round trip through the `layout` column *and* the
//!    `custom_fields` projection — a `Custom` element's config is the
//!    `CustomField` JSON verbatim, so a file field is one of the things the
//!    legacy pair carries fully rather than drops.
//! 2. `create_submission` reads the layout off the row and exchanges a sealed
//!    receipt for a URL. That the *stored* row holds the URL — and therefore
//!    that the backend payload does — is the whole point of the feature.
//! 3. The receipt is the trust boundary. A raw URL, a receipt minted for
//!    another field, and a tampered token all have to be rejected by the real
//!    submission path, not just by `receipt::open` in isolation.
//! 4. A hidden file field's answer is dropped before it is ever resolved, so a
//!    conditional upload doesn't mint a URL nobody stores.
//!
//! Unlike the other integration suites, these tests write **global** state:
//! `storage_provider` holds one active row, so a fixture provider necessarily
//! replaces whatever the database had. Each test restores the previous row on
//! the way out, and the fixture is sealed with the deployment's own
//! `ENCRYPTION_KEY` when it is set, so an interrupted run degrades to "storage
//! points at the test bucket" rather than "storage can no longer be decrypted".
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test file_uploads -- --ignored --nocapture
//! ```

use std::sync::Arc;

use open_relay_core::backend::BackendRegistry;
use open_relay_core::crypto::SecretCipher;
use open_relay_core::forms::{
    Condition, ConditionOp, CustomField, CustomFieldType, FieldWidth, FormElement, MatchMode,
    NewForm, StandardElement, StandardFieldConfig, VisibilityRule, service,
};
use open_relay_core::storage::{FileStoreFactory, S3Factory, StorageRegistry, receipt};
use open_relay_core::storage_config::{UpsertStorageConfig, service as storage_service};
use open_relay_core::submissions::{NewSubmissionPayload, service as submissions};
use sea_orm::Database;
use serde_json::{Value as JsonValue, json};

/// Prefer the deployment's own key when one is in the environment.
///
/// These tests write the **singleton** `storage_provider` row (see
/// `configure_storage`), so a row sealed with a key the local server doesn't
/// have would leave that server unable to decrypt its own storage config —
/// including if a test panics before the restore below runs. Using
/// `ENCRYPTION_KEY` when it is set means the worst residue is a provider
/// pointing at the compose `storage` profile's MinIO, which is exactly what a
/// developer running these tests already has.
fn cipher() -> &'static SecretCipher {
    static CIPHER: std::sync::OnceLock<SecretCipher> = std::sync::OnceLock::new();
    CIPHER.get_or_init(|| match std::env::var("ENCRYPTION_KEY") {
        Ok(key) => SecretCipher::from_base64_key(&key)
            .expect("ENCRYPTION_KEY must be base64 for 32 bytes"),
        Err(_) => SecretCipher::from_key_bytes(&[42u8; 32]).unwrap(),
    })
}

fn storage() -> &'static StorageRegistry {
    static REGISTRY: std::sync::OnceLock<StorageRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut r = StorageRegistry::new();
        r.register_factory(Arc::new(S3Factory::new()));
        r
    })
}

fn uploads() -> submissions::UploadContext<'static> {
    submissions::UploadContext {
        cipher: cipher(),
        registry: storage(),
    }
}

fn registry() -> BackendRegistry {
    let mut r = BackendRegistry::new();
    r.register_static(Arc::new(open_relay_core::backend::openrelay::OpenRelayBackend));
    r
}

fn new_form(slug: &str, layout: Vec<FormElement>) -> NewForm {
    NewForm {
        name: "File upload integration".into(),
        display_name: None,
        slug: Some(slug.into()),
        standard_fields: None,
        custom_fields: vec![],
        layout: Some(layout),
        backends: None,
        tags: vec![],
        reps: vec![],
        source_params: vec![],
        post_submission_action: Default::default(),
        progress_indicator: Default::default(),
        theme_id: None,
        metadata: None,
    }
}

fn file_field(key: &str, required: bool, accept: Vec<String>, max_size_mb: u32) -> FormElement {
    FormElement::Custom(CustomField {
        key: key.into(),
        label: format!("Label {key}"),
        kind: CustomFieldType::File { accept, max_size_mb },
        required,
        placeholder: None,
        help_text: None,
        position: 0,
        width: FieldWidth::Full,
        default_value: None,
        visible_when: None,
    })
}

fn email_element() -> FormElement {
    FormElement::Standard(StandardElement {
        required: true,
        ..StandardElement::from_legacy("email", &StandardFieldConfig::default_enabled())
    })
}

fn payload(pairs: &[(&str, JsonValue)]) -> NewSubmissionPayload {
    NewSubmissionPayload(
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect(),
    )
}

/// Configure a public-visibility S3 provider so `stored_url` is a plain,
/// deterministic URL we can assert on without reaching the network.
///
/// **This writes the singleton active provider**, so it clobbers whatever the
/// database already had. Every test that calls it pairs it with
/// [`restore_storage`], and [`cipher`] uses the deployment's own key, so an
/// interrupted run leaves something usable rather than an undecryptable row.
/// Returns the previous row, if any, to hand back to `restore_storage`.
async fn configure_storage(
    db: &sea_orm::DatabaseConnection,
) -> Option<entity::storage_provider::Model> {
    let previous = storage_service::get_active(db).await.expect("read existing");
    storage_service::upsert(
        db,
        storage(),
        cipher(),
        UpsertStorageConfig {
            kind: "s3".into(),
            name: "Test bucket".into(),
            config: json!({
                "bucket": "uploads-test",
                "region": "us-east-1",
                "access_key_id": "AKIAEXAMPLE",
                "secret_access_key": "s3cret",
                "visibility": "public",
            }),
        },
    )
    .await
    .expect("configure storage");
    previous
}

/// Put back whatever provider was configured before the test ran, so the
/// suite doesn't leave a developer's storage settings pointing at a fixture.
async fn restore_storage(
    db: &sea_orm::DatabaseConnection,
    previous: Option<entity::storage_provider::Model>,
) {
    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
    match previous {
        Some(prev) => {
            let current = storage_service::get_active(db).await.expect("read current");
            if let Some(current) = current {
                // Write the original config back verbatim rather than
                // through `upsert`: it is already ciphertext under whatever
                // key wrote it, and `upsert` would try to decrypt it to
                // validate — which fails if that key isn't the one we hold.
                let mut active: entity::storage_provider::ActiveModel = current.into();
                active.kind = ActiveValue::Set(prev.kind);
                active.name = ActiveValue::Set(prev.name);
                active.config = ActiveValue::Set(prev.config);
                active.is_active = ActiveValue::Set(prev.is_active);
                active.update(db).await.expect("restore provider");
            }
        }
        None => {
            // There was nothing configured before; leave it that way.
            let _ = entity::storage_provider::Entity::delete_many()
                .exec(db)
                .await;
        }
    }
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn file_field_round_trips_and_a_receipt_becomes_a_url() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("file-it-{}", std::process::id());
    let previous = configure_storage(&db).await;

    let layout = vec![
        email_element(),
        file_field("resume", true, vec![".pdf".into()], 5),
    ];
    let created = service::create_form(&db, &reg, 1, new_form(&slug, layout.clone()))
        .await
        .expect("create form");

    // 1. The variant and its payload survive the layout column verbatim.
    let stored = service::layout_from_model(&created).expect("layout");
    let kind = stored
        .iter()
        .find_map(|el| match el {
            FormElement::Custom(c) if c.key == "resume" => Some(c.kind.clone()),
            _ => None,
        })
        .expect("resume field");
    assert_eq!(
        kind,
        CustomFieldType::File {
            accept: vec![".pdf".into()],
            max_size_mb: 5
        }
    );

    // 2. …and rides along in the legacy projection, so a bundle cached before
    //    `layout` existed still renders it (as a file input it understands, or
    //    not at all — but the server's view of the form is identical).
    let legacy = service::parse_custom_fields(&created.custom_fields).expect("legacy pair");
    assert!(
        legacy.iter().any(|f| f.key == "resume" && f.kind.is_file()),
        "a file field must survive `legacy_from_layout` like any other custom field"
    );

    // 3. A sealed receipt is exchanged for the object's URL on submit.
    let token = receipt::seal(cipher(), created.id, "resume", "forms/1/2026/09/abc/cv.pdf")
        .expect("seal");
    let inserted = submissions::create_submission(
        &db,
        &created,
        payload(&[
            ("email", json!("a@b.co")),
            ("resume", json!(token)),
        ]),
        &uploads(),
    )
    .await
    .expect("submission accepted");

    let stored_value = inserted
        .custom_data
        .get("resume")
        .and_then(JsonValue::as_str)
        .expect("resume stored");
    assert_eq!(
        stored_value,
        "https://uploads-test.s3.us-east-1.amazonaws.com/forms/1/2026/09/abc/cv.pdf",
        "the stored value must be the object URL, not the receipt"
    );

    // 4. And the URL — not the token — is what a backend is handed.
    let delivery = submissions::delivery_data(&inserted);
    assert_eq!(delivery.get("resume").and_then(JsonValue::as_str), Some(stored_value));

    service::delete_form(&db, created.id).await.ok();
    restore_storage(&db, previous).await;
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn only_a_valid_receipt_is_accepted() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("file-it-reject-{}", std::process::id());
    let previous = configure_storage(&db).await;

    let layout = vec![
        email_element(),
        file_field("resume", false, vec![], 5),
        file_field("cover_letter", false, vec![], 5),
    ];
    let created = service::create_form(&db, &reg, 1, new_form(&slug, layout))
        .await
        .expect("create form");

    // The attack the receipt exists to stop: a bare URL as the field value.
    // Without the exchange this would be delivered to a CRM as though a
    // visitor had uploaded it.
    let err = submissions::create_submission(
        &db,
        &created,
        payload(&[
            ("email", json!("a@b.co")),
            ("resume", json!("https://evil.example/malware.exe")),
        ]),
        &uploads(),
    )
    .await
    .expect_err("a raw URL must be rejected");
    assert!(format!("{err}").contains("resume"), "{err}");

    // A receipt minted for a different field on the same form.
    let wrong_field = receipt::seal(cipher(), created.id, "cover_letter", "forms/1/x.pdf").unwrap();
    assert!(
        submissions::create_submission(
            &db,
            &created,
            payload(&[("email", json!("a@b.co")), ("resume", json!(wrong_field))]),
            &uploads(),
        )
        .await
        .is_err(),
        "a receipt is bound to one field"
    );

    // A receipt minted for a different form.
    let wrong_form = receipt::seal(cipher(), created.id + 9999, "resume", "forms/1/x.pdf").unwrap();
    assert!(
        submissions::create_submission(
            &db,
            &created,
            payload(&[("email", json!("a@b.co")), ("resume", json!(wrong_form))]),
            &uploads(),
        )
        .await
        .is_err(),
        "a receipt is bound to one form"
    );

    // A tampered token.
    let mut tampered = receipt::seal(cipher(), created.id, "resume", "forms/1/x.pdf").unwrap();
    let last = tampered.pop().unwrap();
    tampered.push(if last == 'A' { 'B' } else { 'A' });
    assert!(
        submissions::create_submission(
            &db,
            &created,
            payload(&[("email", json!("a@b.co")), ("resume", json!(tampered))]),
            &uploads(),
        )
        .await
        .is_err(),
        "AEAD must reject a modified receipt"
    );

    // An unanswered optional file field is simply absent, not an error.
    let ok = submissions::create_submission(
        &db,
        &created,
        payload(&[("email", json!("a@b.co")), ("resume", json!(""))]),
        &uploads(),
    )
    .await
    .expect("an empty optional file field is fine");
    assert!(ok.custom_data.get("resume").is_none());

    service::delete_form(&db, created.id).await.ok();
    restore_storage(&db, previous).await;
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn a_hidden_file_field_is_dropped_before_it_is_resolved() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("file-it-hidden-{}", std::process::id());
    let previous = configure_storage(&db).await;

    // `resume` is only shown when `has_resume` is "Yes".
    let mut gated = file_field("resume", true, vec![], 5);
    if let FormElement::Custom(c) = &mut gated {
        c.visible_when = Some(VisibilityRule {
            match_mode: MatchMode::All,
            conditions: vec![Condition {
                field: "has_resume".into(),
                op: ConditionOp::Equals,
                value: Some("Yes".into()),
            }],
        });
    }
    let layout = vec![
        email_element(),
        FormElement::Custom(CustomField {
            key: "has_resume".into(),
            label: "Attach a résumé?".into(),
            kind: CustomFieldType::Select {
                options: vec!["Yes".into(), "No".into()],
            },
            required: true,
            placeholder: None,
            help_text: None,
            position: 0,
            width: FieldWidth::Full,
            default_value: None,
            visible_when: None,
        }),
        gated,
    ];
    let created = service::create_form(&db, &reg, 1, new_form(&slug, layout))
        .await
        .expect("create form");

    // A cached bundle that predates rules submits the file anyway. The answer
    // is dropped rather than resolved — and, critically, the *required* file
    // field doesn't block the submission, because it isn't shown.
    let token = receipt::seal(cipher(), created.id, "resume", "forms/1/x.pdf").unwrap();
    let inserted = submissions::create_submission(
        &db,
        &created,
        payload(&[
            ("email", json!("a@b.co")),
            ("has_resume", json!("No")),
            ("resume", json!(token)),
        ]),
        &uploads(),
    )
    .await
    .expect("hidden required file field must not block the submission");
    assert!(
        inserted.custom_data.get("resume").is_none(),
        "a hidden file answer is dropped, not stored"
    );

    service::delete_form(&db, created.id).await.ok();
    restore_storage(&db, previous).await;
}

/// End-to-end against a real S3 implementation.
///
/// The unit tests pin the signature against AWS's published canonical
/// request, but only a real store proves the whole assembly — host style,
/// path encoding, the signed `Content-Type`/`Content-Length`, and the public
/// URL `stored_url` builds. A wrong signature is otherwise invisible until
/// production answers `SignatureDoesNotMatch`.
///
/// ```text
/// docker compose -f infra/docker-compose.yml --profile storage up -d
/// MINIO_URL=http://127.0.0.1:9000 \
///   cargo test -p open-relay-core --test file_uploads -- --ignored minio
/// ```
#[tokio::test]
#[ignore = "requires a live MinIO (docker compose --profile storage up -d)"]
async fn minio_accepts_our_presigned_upload_and_serves_it_back() {
    let endpoint = std::env::var("MINIO_URL").unwrap_or_else(|_| "http://127.0.0.1:9000".into());
    let factory = S3Factory::new();
    let store = factory
        .build(&json!({
            "bucket": "open-relay-uploads",
            "region": "us-east-1",
            "endpoint": endpoint,
            "access_key_id": "openrelay",
            "secret_access_key": "openrelay",
            "force_path_style": true,
            "visibility": "public",
        }))
        .expect("build store");

    // The connection test itself: presign, PUT, delete.
    store.check().await.expect("connection test against MinIO");

    // Now the real shape — a presigned PUT the browser would perform, with
    // the exact headers we hand the client.
    let key = format!("forms/1/2026/09/{}/hello.txt", uuid_like());
    let body = b"open-relay upload round trip".to_vec();
    let presigned = store
        .presign_put(&key, "text/plain", body.len() as u64)
        .await
        .expect("presign");

    let http = reqwest::Client::new();
    let mut req = http.put(&presigned.url).body(body.clone());
    for (name, value) in &presigned.headers {
        req = req.header(name, value);
    }
    let res = req.send().await.expect("PUT reaches MinIO");
    assert!(res.status().is_success(), "PUT failed: {}", res.status());

    // And the URL we record on the submission actually serves the bytes.
    let url = store.stored_url(&key).await.expect("stored url");
    let fetched = http.get(&url).send().await.expect("GET the stored URL");
    assert!(fetched.status().is_success(), "GET failed: {}", fetched.status());
    assert_eq!(fetched.bytes().await.unwrap().as_ref(), body.as_slice());

    // A body that doesn't match the signed Content-Length must be refused —
    // this is what actually enforces a field's size limit, so it is worth
    // proving against a real implementation rather than assuming.
    let presigned = store
        .presign_put(&format!("{key}.2"), "text/plain", 10)
        .await
        .expect("presign");
    let mut req = http
        .put(&presigned.url)
        .body(vec![b'x'; 5000]);
    for (name, value) in &presigned.headers {
        req = req.header(name, value);
    }
    let res = req.send().await.expect("oversized PUT reaches MinIO");
    assert!(
        !res.status().is_success(),
        "an oversized body must fail the signature check, got {}",
        res.status()
    );
}

fn uuid_like() -> String {
    format!("{:x}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos())
}
