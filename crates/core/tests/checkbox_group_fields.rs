//! Read-after-write check for checkbox-group fields, end to end.
//!
//! A `checkboxes` answer is the one array in `custom_data`, so this pins what
//! only a live database shows: that the options survive in both the `layout`
//! column and the legacy `custom_fields` projection, that the stored answer is
//! an array in the author's option order, and that a visibility rule reads that
//! array as a set — the same way `visibility.ts` does in the browser.
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test checkbox_group_fields -- --ignored --nocapture
//! ```

use open_relay_core::backend::BackendRegistry;
use open_relay_core::forms::{
    Condition, ConditionOp, CustomField, CustomFieldType, FieldWidth, FormElement, MatchMode,
    NewForm, VisibilityRule, service,
};
use open_relay_core::submissions::{NewSubmissionPayload, service as submissions};
use sea_orm::Database;
use serde_json::{Value as JsonValue, json};

/// Upload context for a form with no `file` fields; never consulted.
fn no_uploads() -> submissions::UploadContext<'static> {
    static CIPHER: std::sync::OnceLock<open_relay_core::crypto::SecretCipher> =
        std::sync::OnceLock::new();
    static REGISTRY: std::sync::OnceLock<open_relay_core::storage::StorageRegistry> =
        std::sync::OnceLock::new();
    submissions::UploadContext {
        cipher: CIPHER.get_or_init(|| {
            open_relay_core::crypto::SecretCipher::from_key_bytes(&[1u8; 32]).unwrap()
        }),
        registry: REGISTRY.get_or_init(open_relay_core::storage::StorageRegistry::new),
    }
}

fn registry() -> BackendRegistry {
    let mut r = BackendRegistry::new();
    r.register_static(std::sync::Arc::new(
        open_relay_core::backend::openrelay::OpenRelayBackend,
    ));
    r
}

fn new_form(slug: &str, layout: Vec<FormElement>) -> NewForm {
    NewForm {
        name: "Checkbox group integration".into(),
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

fn custom(
    key: &str,
    kind: CustomFieldType,
    required: bool,
    visible_when: Option<VisibilityRule>,
) -> FormElement {
    FormElement::Custom(CustomField {
        key: key.into(),
        label: format!("Label {key}"),
        kind,
        required,
        placeholder: None,
        help_text: None,
        position: 0,
        width: FieldWidth::Full,
        default_value: None,
        visible_when,
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

fn equipment() -> CustomFieldType {
    CustomFieldType::Checkboxes {
        options: vec![
            "Barcode Scanners".into(),
            "Mobile Computers".into(),
            "Label Printers".into(),
            "Label Rewinders".into(),
            "Barcode Verifiers".into(),
        ],
    }
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn a_checkbox_group_round_trips_and_stores_an_array() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("checkboxes-it-{}", std::process::id());

    // A follow-up shown only to someone who ticked label printers.
    let layout = vec![
        custom("equipment", equipment(), true, None),
        custom(
            "printer_volume",
            CustomFieldType::Text,
            false,
            Some(VisibilityRule {
                match_mode: MatchMode::All,
                conditions: vec![Condition {
                    field: "equipment".into(),
                    op: ConditionOp::Equals,
                    value: Some("Label Printers".into()),
                }],
            }),
        ),
    ];

    let created = service::create_form(&db, &reg, 1, new_form(&slug, layout))
        .await
        .expect("create");

    // 1. Options survive on the layout and in the legacy projection.
    let stored = service::layout_from_model(&created).expect("parse layout");
    assert!(matches!(&stored[0], FormElement::Custom(c) if c.kind == equipment()));
    let public = service::public_dto_from_model(created.clone()).expect("public dto");
    assert!(
        public
            .custom_fields
            .iter()
            .any(|f| f.key == "equipment" && f.kind == equipment()),
        "the checkbox group rides along in the legacy column"
    );

    // 2. Stored as an array in option order; the rule matches an element.
    let with_printers = submissions::create_submission(
        &db,
        &created,
        payload(&[
            ("equipment", json!(["Label Rewinders", "Mobile Computers", "Label Printers"])),
            ("printer_volume", json!("500/day")),
        ]),
        &no_uploads(),
    )
    .await
    .expect("a valid selection");
    assert_eq!(
        with_printers.custom_data["equipment"],
        json!(["Mobile Computers", "Label Printers", "Label Rewinders"])
    );
    assert_eq!(with_printers.custom_data["printer_volume"], json!("500/day"));

    // 3. Without printers ticked, the follow-up is hidden and its answer dropped.
    let without = submissions::create_submission(
        &db,
        &created,
        payload(&[
            ("equipment", json!(["Barcode Scanners"])),
            ("printer_volume", json!("dropped")),
        ]),
        &no_uploads(),
    )
    .await
    .expect("a selection without printers");
    assert!(
        without.custom_data.get("printer_volume").is_none_or(JsonValue::is_null),
        "the follow-up was hidden, so its answer is discarded"
    );

    // 4. A pre-`layout` bundle posts one string from a text box.
    let legacy = submissions::create_submission(
        &db,
        &created,
        payload(&[("equipment", json!("Barcode Verifiers"))]),
        &no_uploads(),
    )
    .await
    .expect("a single exact option");
    assert_eq!(legacy.custom_data["equipment"], json!(["Barcode Verifiers"]));

    // 5. Unknown options, and nothing ticked on a required group, are refused.
    for bad in [json!(["Lasers"]), json!([]), json!("Scanners, Printers")] {
        let res = submissions::create_submission(
            &db,
            &created,
            payload(&[("equipment", bad.clone())]),
            &no_uploads(),
        )
        .await;
        assert!(res.is_err(), "{bad} must be rejected");
    }

    // 6. A group with no options can't be saved.
    let empty = service::create_form(
        &db,
        &reg,
        1,
        new_form(
            &format!("{slug}-bad"),
            vec![custom(
                "equipment",
                CustomFieldType::Checkboxes { options: vec![] },
                false,
                None,
            )],
        ),
    )
    .await;
    assert!(empty.is_err());
}
