//! Read-after-write check for conditional-field visibility, end to end.
//!
//! Three things about this feature only break in ways a live database can
//! reveal, so unit tests over the pure functions aren't enough:
//!
//! 1. A rule lives inside the `layout` JSON column. The projection onto the
//!    legacy pair drops a *standard* element's rule and carries a *custom*
//!    field's (a `Custom` element's config is the `CustomField` JSON verbatim),
//!    and both halves have to survive a real write and read back.
//! 2. `create_submission` reads the layout off the row, not the DTO, and prunes
//!    fields the rules say weren't shown. That the pruning reaches the *stored*
//!    row — and therefore the backend payload — is the whole point.
//! 3. A legacy-only PATCH has no vocabulary for rules but moves elements
//!    around, so it must never 400 on a rule it cannot see.
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test conditional_fields -- --ignored --nocapture
//! ```

use open_relay_core::backend::BackendRegistry;
use open_relay_core::forms::{
    Condition, ConditionOp, CustomField, CustomFieldType, FieldWidth, FormElement, MatchMode,
    NewForm, StandardElement, StandardFieldConfig, StandardFieldsConfig, UpdateForm,
    VisibilityRule, service,
};
use open_relay_core::submissions::{NewSubmissionPayload, service as submissions};
use sea_orm::Database;
use serde_json::{Value as JsonValue, json};

fn registry() -> BackendRegistry {
    let mut r = BackendRegistry::new();
    r.register_static(std::sync::Arc::new(
        open_relay_core::backend::openrelay::OpenRelayBackend,
    ));
    r
}

fn new_form(slug: &str, layout: Vec<FormElement>) -> NewForm {
    NewForm {
        name: "Conditional fields integration".into(),
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
        metadata: None,
    }
}

fn when(field: &str, op: ConditionOp, value: Option<&str>) -> VisibilityRule {
    VisibilityRule {
        match_mode: MatchMode::All,
        conditions: vec![Condition {
            field: field.into(),
            op,
            value: value.map(str::to_string),
        }],
    }
}

fn radio(key: &str, options: &[&str]) -> FormElement {
    FormElement::Custom(CustomField {
        key: key.into(),
        label: "Is the billing and shipping address the same?".into(),
        kind: CustomFieldType::Radio {
            options: options.iter().map(|o| (*o).to_string()).collect(),
        },
        required: true,
        placeholder: None,
        help_text: None,
        position: 0,
        width: FieldWidth::Full,
        default_value: None,
        visible_when: None,
    })
}

fn custom_when(key: &str, rule: Option<VisibilityRule>) -> FormElement {
    FormElement::Custom(CustomField {
        key: key.into(),
        label: format!("Label {key}"),
        kind: CustomFieldType::Text,
        required: true,
        placeholder: None,
        help_text: None,
        position: 0,
        width: FieldWidth::Full,
        default_value: None,
        visible_when: rule,
    })
}

fn standard_when(key: &str, required: bool, rule: Option<VisibilityRule>) -> FormElement {
    FormElement::Standard(StandardElement {
        required,
        visible_when: rule,
        ..StandardElement::from_legacy(key, &StandardFieldConfig::default_enabled())
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

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn conditional_fields_survive_a_round_trip_and_prune_on_submit() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("conditional-it-{}", std::process::id());

    // The shape from the reference screenshots, minus the bulk: one controlling
    // radio, then a billing block shown only when the answer is "No".
    let layout = vec![
        radio("same_address", &["Yes", "No"]),
        standard_when(
            "email",
            true,
            None,
        ),
        // A required standard field behind a rule — the case that would 400 a
        // real visitor if the server stayed layout-blind.
        standard_when(
            "city",
            true,
            Some(when("same_address", ConditionOp::Equals, Some("No"))),
        ),
        custom_when(
            "billing_note",
            Some(when("same_address", ConditionOp::Equals, Some("No"))),
        ),
    ];

    // 1. A rule survives the write and comes back on both read paths.
    let created = service::create_form(&db, &reg, 1, new_form(&slug, layout.clone()))
        .await
        .expect("create");
    let stored = service::layout_from_model(&created).expect("parse layout");
    assert_eq!(
        stored[2].visible_when(),
        Some(&when("same_address", ConditionOp::Equals, Some("No"))),
        "a standard element's rule must survive the write"
    );

    let admin = service::dto_from_model(&db, created.clone())
        .await
        .expect("admin dto");
    assert_eq!(admin.layout[3].visible_when().unwrap().conditions[0].field, "same_address");
    let public = service::public_dto_from_model(created.clone()).expect("public dto");
    assert_eq!(public.layout[2].visible_when(), stored[2].visible_when());

    // 2. The projection is lossy in exactly one direction. A standard element's
    //    rule has nowhere to live in `StandardFieldConfig`; a custom field's is
    //    stored verbatim, so it rides along in `custom_fields`. `shape()`-style
    //    key comparisons can't see either, so assert on the rules directly.
    assert!(
        public.standard_fields.city.enabled && public.standard_fields.city.required,
        "the legacy projection still reports the field as enabled + required"
    );
    assert!(
        public.custom_fields.iter().any(|f| f.key == "billing_note"
            && f.visible_when.as_ref().is_some_and(|r| r.conditions[0].field == "same_address")),
        "a custom field's rule rides along in the legacy column"
    );

    // 3. Answering "No" makes the block visible, so `required` is enforced …
    let missing = submissions::create_submission(
        &db,
        &created,
        payload(&[
            ("same_address", json!("No")),
            ("email", json!("a@b.co")),
        ]),
    )
    .await;
    assert!(missing.is_err(), "a *visible* required field is still required");

    let visible = submissions::create_submission(
        &db,
        &created,
        payload(&[
            ("same_address", json!("No")),
            ("email", json!("a@b.co")),
            ("city", json!("Norwalk")),
            ("billing_note", json!("ring the bell")),
        ]),
    )
    .await
    .expect("a fully answered visible block is accepted");
    assert_eq!(visible.city.as_deref(), Some("Norwalk"));

    // 4. … and answering "Yes" hides it: `required` is not enforced, and a
    //    value sent anyway (a cached bundle, or a visitor who changed their
    //    mind) is dropped from the stored row and so from the backend payload.
    let hidden = submissions::create_submission(
        &db,
        &created,
        payload(&[
            ("same_address", json!("Yes")),
            ("email", json!("a@b.co")),
            ("city", json!("Stale City")),
            ("billing_note", json!("stale note")),
        ]),
    )
    .await
    .expect("a hidden required field must not block the submission");
    assert_eq!(hidden.city, None, "a hidden standard value is not stored");
    assert_eq!(
        hidden.custom_data,
        json!({ "same_address": "Yes" }),
        "the controller is kept; only the block it hid is dropped"
    );
    let delivered = submissions::delivery_data(&hidden);
    assert!(delivered.get("city").is_none() && delivered.get("billing_note").is_none());

    // 5. An invalid rule is rejected, and the stored layout is not clobbered.
    let bad = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            layout: Some(vec![
                custom_when("orphan", Some(when("nope", ConditionOp::IsNotEmpty, None))),
            ]),
            ..Default::default()
        },
    )
    .await;
    assert!(bad.is_err(), "a rule naming an unknown field is a 400");
    let reread = service::find_by_id(&db, created.id).await.expect("find").expect("present");
    assert_eq!(
        service::layout_from_model(&reread).unwrap().len(),
        layout.len(),
        "a rejected write leaves the stored layout alone"
    );

    // 6. A legacy-only PATCH must never 400 on a rule it cannot express. Here
    //    it disables `city`, stranding nothing, and enables `phone`, which pass
    //    2 inserts at its catalogue position — ahead of the customs. Rules the
    //    move strands are repaired to "unconditional" rather than rejected.
    let mut sf = StandardFieldsConfig::all_disabled();
    sf.email = StandardFieldConfig::default_enabled();
    sf.phone = StandardFieldConfig::default_enabled();
    let patched = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            standard_fields: Some(sf),
            ..Default::default()
        },
    )
    .await
    .expect("a legacy PATCH must not 400 on rules it never saw");
    let patched_layout = service::layout_from_model(&patched).expect("parse layout");
    assert!(
        patched_layout.iter().any(|e| e.field_key() == Some("phone")),
        "the legacy write still took effect"
    );
    assert!(
        patched_layout.iter().any(|e| e.field_key() == Some("billing_note")),
        "the custom field survived"
    );
    assert!(
        service::validate_layout(&patched_layout).is_ok(),
        "a legacy PATCH must never leave the layout in a state it can't re-save"
    );

    if std::env::var("KEEP_FIXTURE").is_ok() {
        eprintln!("KEEP_FIXTURE: form id={} slug={}", created.id, slug);
    } else {
        service::delete_form(&db, created.id).await.expect("cleanup");
    }
}
