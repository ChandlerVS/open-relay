//! Read-after-write check for star-rating fields, end to end.
//!
//! Two things only a live database shows: that a rating's `max` survives in
//! both the `layout` column and the legacy `custom_fields` projection (a
//! `Custom` element's config is the `CustomField` JSON verbatim), and that the
//! stored answer is an integer — which is what lets a visibility rule read it
//! as `"7"` on the server exactly as the renderer does.
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test rating_fields -- --ignored --nocapture
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
        name: "Rating fields integration".into(),
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

fn custom(key: &str, kind: CustomFieldType, visible_when: Option<VisibilityRule>) -> FormElement {
    FormElement::Custom(CustomField {
        key: key.into(),
        label: format!("Label {key}"),
        kind,
        required: false,
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

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn a_rating_round_trips_and_stores_an_integer() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("rating-it-{}", std::process::id());

    // A follow-up question shown only for a low score — the rule operand is the
    // string the renderer holds, so it only matches if the server agrees.
    let layout = vec![
        custom("score", CustomFieldType::Rating { max: 10 }, None),
        custom(
            "why_low",
            CustomFieldType::Text,
            Some(VisibilityRule {
                match_mode: MatchMode::All,
                conditions: vec![Condition {
                    field: "score".into(),
                    op: ConditionOp::Equals,
                    value: Some("2".into()),
                }],
            }),
        ),
    ];

    let created = service::create_form(&db, &reg, 1, new_form(&slug, layout))
        .await
        .expect("create");

    // 1. `max` survives on the layout and in the legacy projection.
    let stored = service::layout_from_model(&created).expect("parse layout");
    assert!(matches!(
        &stored[0],
        FormElement::Custom(c) if c.kind == CustomFieldType::Rating { max: 10 }
    ));
    let public = service::public_dto_from_model(created.clone()).expect("public dto");
    assert!(
        public
            .custom_fields
            .iter()
            .any(|f| f.key == "score" && f.kind == CustomFieldType::Rating { max: 10 }),
        "the rating rides along in the legacy column"
    );

    // 2. A string answer is stored as an integer.
    let ok = submissions::create_submission(
        &db,
        &created,
        payload(&[("score", json!("7")), ("why_low", json!("dropped"))]),
        &no_uploads(),
    )
    .await
    .expect("a valid rating");
    assert_eq!(ok.custom_data["score"], json!(7));
    assert!(
        ok.custom_data.get("why_low").is_none_or(JsonValue::is_null),
        "7 is not 2, so the follow-up was hidden and its answer dropped"
    );

    // 3. The rule reads the integer the same way the renderer reads "2".
    let low = submissions::create_submission(
        &db,
        &created,
        payload(&[("score", json!(2)), ("why_low", json!("slow"))]),
        &no_uploads(),
    )
    .await
    .expect("a low rating");
    assert_eq!(low.custom_data["why_low"], json!("slow"));

    // 4. Out of range is refused.
    for bad in [json!("11"), json!(0), json!("7.5")] {
        let res = submissions::create_submission(
            &db,
            &created,
            payload(&[("score", bad.clone())]),
            &no_uploads(),
        )
        .await;
        assert!(res.is_err(), "{bad} is not a 1..=10 rating");
    }

    // 5. A star count outside the bounds can't be saved.
    let too_many = service::create_form(
        &db,
        &reg,
        1,
        new_form(
            &format!("{slug}-bad"),
            vec![custom("score", CustomFieldType::Rating { max: 11 }, None)],
        ),
    )
    .await;
    assert!(too_many.is_err());
}
