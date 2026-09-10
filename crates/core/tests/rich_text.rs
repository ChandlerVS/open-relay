//! Read-after-write checks for the `rich_text` layout element.
//!
//! Three properties matter here and none of them are visible from a unit test
//! on `validate_layout` alone:
//!
//! 1. The markdown must come back **byte-identical**. It is the stored source
//!    of truth and the renderer is the only thing that parses it, so a write
//!    path that reserialised or re-escaped it would silently change what every
//!    visitor sees — and would make a pristine form read as edited in the
//!    builder, whose dirty check is a `JSON.stringify` comparison.
//! 2. A default `tone` must not reach the column at all, for the same reason.
//! 3. The block must stay invisible to the legacy projection, so an embed
//!    bundle cached before this element existed keeps rendering a valid form.
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test rich_text -- --ignored --nocapture
//! ```

use open_relay_core::backend::BackendRegistry;
use open_relay_core::forms::{
    Condition, ConditionOp, CustomField, CustomFieldType, FormElement, MatchMode, NewForm,
    RichTextElement, RichTextTone, StandardElement, StandardFieldConfig, StandardFieldsConfig,
    UpdateForm, VisibilityRule, service,
};
use sea_orm::Database;

fn registry() -> BackendRegistry {
    let mut r = BackendRegistry::new();
    r.register_static(std::sync::Arc::new(
        open_relay_core::backend::openrelay::OpenRelayBackend,
    ));
    r
}

fn new_form(slug: &str, layout: Vec<FormElement>) -> NewForm {
    NewForm {
        name: "Rich text integration".into(),
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

fn std_el(key: &str) -> FormElement {
    FormElement::Standard(StandardElement {
        key: key.into(),
        required: false,
        label: None,
        placeholder: None,
        help_text: None,
        width: Default::default(),
        default_value: None,
        input_override: None,
        visible_when: None,
    })
}

fn custom(key: &str) -> CustomField {
    CustomField {
        key: key.into(),
        label: key.into(),
        kind: CustomFieldType::Text,
        required: false,
        position: 0,
        placeholder: None,
        help_text: None,
        width: Default::default(),
        default_value: None,
        visible_when: None,
    }
}

/// The reference case this feature exists for: bold lead-ins, a link, a list.
const COPY: &str = "**Credit Card**: once your order is processed, an email is sent.\n\
\n\
We use a third-party provider. Add these to your safe list:\n\
\n\
- one@example.com\n\
- two@example.com\n\
\n\
You can [fill it out now here](https://example.com/apply?a=1&b=2).";

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn rich_text_round_trips_byte_identically_and_stays_out_of_the_projection() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("richtext-it-{}", std::process::id());

    let layout = vec![
        std_el("email"),
        FormElement::RichText(RichTextElement {
            markdown: COPY.into(),
            tone: RichTextTone::Warning,
            visible_when: None,
        }),
        FormElement::Custom(custom("colour")),
    ];

    let created = service::create_form(&db, &reg, 1, new_form(&slug, layout))
        .await
        .expect("create");

    // 1. Byte-identical markdown, through the public read path an embed uses.
    let public = service::public_dto_from_model(created.clone()).unwrap();
    let block = public
        .layout
        .iter()
        .find_map(|e| match e {
            FormElement::RichText(r) => Some(r),
            _ => None,
        })
        .expect("the block survived the write");
    assert_eq!(
        block.markdown, COPY,
        "markdown must round-trip verbatim — it is the source of truth and the \
         renderer is the only thing that parses it"
    );
    assert_eq!(block.tone, RichTextTone::Warning);

    // 2. Invisible to the legacy projection, exactly like a heading. This is
    //    what keeps a cached embed bundle rendering a valid form.
    assert!(public.standard_fields.email.enabled);
    assert_eq!(
        public.custom_fields.len(),
        1,
        "a rich-text block must not leak into custom_fields"
    );
    assert_eq!(public.custom_fields[0].key, "colour");

    service::delete_form(&db, created.id).await.ok();
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn a_default_tone_never_reaches_the_column() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("richtext-tone-{}", std::process::id());

    let created = service::create_form(
        &db,
        &reg,
        1,
        new_form(
            &slug,
            vec![FormElement::RichText(RichTextElement {
                markdown: "plain".into(),
                tone: RichTextTone::Normal,
                visible_when: None,
            })],
        ),
    )
    .await
    .expect("create");

    // Asserted against the raw JSON column, which is the only place this can
    // actually be checked: "never configured" and "set back to normal" have to
    // be the same bytes, or the builder's dirty check lies on every load.
    let raw = created.layout.clone().expect("layout column");
    let text = serde_json::to_string(&raw).unwrap();
    assert!(
        !text.contains("tone"),
        "a default tone must not be serialised: {text}"
    );

    // And a non-default one must be.
    let updated = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            layout: Some(vec![FormElement::RichText(RichTextElement {
                markdown: "plain".into(),
                tone: RichTextTone::Danger,
                visible_when: None,
            })]),
            ..Default::default()
        },
    )
    .await
    .expect("update");
    let text = serde_json::to_string(&updated.layout.clone().unwrap()).unwrap();
    assert!(text.contains("\"tone\":\"danger\""), "{text}");

    service::delete_form(&db, created.id).await.ok();
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn a_legacy_write_repairs_a_stranded_rule_instead_of_rejecting_it() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("richtext-rule-{}", std::process::id());

    // A block conditional on a custom field that a later legacy write removes.
    let layout = vec![
        FormElement::Custom(custom("plan")),
        FormElement::RichText(RichTextElement {
            markdown: "shown only for a plan".into(),
            tone: RichTextTone::Normal,
            visible_when: Some(VisibilityRule {
                match_mode: MatchMode::All,
                conditions: vec![Condition {
                    field: "plan".into(),
                    op: ConditionOp::Equals,
                    value: Some("pro".into()),
                }],
            }),
        }),
    ];
    let created = service::create_form(&db, &reg, 1, new_form(&slug, layout))
        .await
        .expect("create");

    // A legacy-only write: this caller has no vocabulary for rules and cannot
    // see the one it is about to strand. Rejecting would be unactionable, so
    // the element must survive and simply become unconditional.
    let updated = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            standard_fields: Some(StandardFieldsConfig {
                email: StandardFieldConfig {
                    enabled: true,
                    required: false,
                    label: None,
                },
                ..Default::default()
            }),
            custom_fields: Some(vec![]),
            ..Default::default()
        },
    )
    .await
    .expect("legacy write must repair, not reject");

    let public = service::public_dto_from_model(updated.clone()).unwrap();
    let block = public
        .layout
        .iter()
        .find_map(|e| match e {
            FormElement::RichText(r) => Some(r),
            _ => None,
        })
        .expect("the block must survive a legacy write");
    assert_eq!(block.markdown, "shown only for a plan");
    assert!(
        block.visible_when.is_none(),
        "the stranded rule must be stripped, leaving the block unconditional"
    );

    service::delete_form(&db, created.id).await.ok();
}
