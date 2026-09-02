//! Read-after-write compat check for the form `layout` column.
//!
//! The production hazard this guards: embed bundles cached on third-party host
//! pages read `standard_fields`/`custom_fields` and can never be upgraded. If a
//! save through the layout path ever let those columns go stale, every such
//! bundle would render the wrong form — or an empty one that rejects every
//! submission.
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test layout_projection -- --ignored --nocapture
//! ```

use open_relay_core::backend::BackendRegistry;
use open_relay_core::forms::{
    CustomField, CustomFieldType, FieldWidth, FormElement, HeadingElement, NewForm,
    PageBreakElement, StandardElement, UpdateForm, service,
};
use sea_orm::Database;

fn registry() -> BackendRegistry {
    let mut r = BackendRegistry::new();
    r.register_static(std::sync::Arc::new(
        open_relay_core::backend::openrelay::OpenRelayBackend,
    ));
    r
}

fn custom(key: &str) -> CustomField {
    CustomField {
        key: key.into(),
        label: format!("Label {key}"),
        kind: CustomFieldType::Text,
        required: false,
        placeholder: None,
        help_text: None,
        position: 0,
        width: FieldWidth::Full,
        default_value: None,
    }
}

fn standard(key: &str, required: bool) -> FormElement {
    FormElement::Standard(StandardElement {
        key: key.into(),
        required,
        label: None,
        placeholder: None,
        help_text: None,
        width: FieldWidth::Full,
        default_value: None,
        input_override: None,
    })
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn saving_a_layout_keeps_the_legacy_columns_in_step() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();

    let slug = format!("layout-it-{}", std::process::id());

    // A layout that interleaves a custom field between two standards, and
    // carries decoration the legacy columns cannot express.
    let layout = vec![
        standard("first_name", true),
        FormElement::Custom(custom("how_heard")),
        standard("email", true),
        FormElement::Heading(HeadingElement { text: "More".into(), level: 2 }),
        FormElement::PageBreak(PageBreakElement { title: Some("Step 2".into()) }),
        standard("country", false),
        FormElement::Custom(custom("notes")),
    ];

    let created = service::create_form(
        &db,
        &reg,
        1,
        NewForm {
            name: "Layout integration".into(),
            slug: Some(slug.clone()),
            standard_fields: None,
            custom_fields: vec![],
            layout: Some(layout.clone()),
            backends: None,
            tags: vec![],
            reps: vec![],
            source_params: vec![],
            post_submission_action: Default::default(),
            metadata: None,
        },
    )
    .await
    .expect("create");

    // 1. The legacy columns are a faithful projection, not stale or empty.
    let sf = service::parse_standard_fields(&created.standard_fields).unwrap();
    let cf = service::parse_custom_fields(&created.custom_fields).unwrap();
    assert!(sf.first_name.enabled && sf.first_name.required);
    assert!(sf.email.enabled && sf.email.required);
    assert!(sf.country.enabled && !sf.country.required);
    assert!(!sf.phone.enabled, "untouched keys stay disabled");
    assert_eq!(
        cf.iter().map(|c| (c.key.as_str(), c.position)).collect::<Vec<_>>(),
        vec![("how_heard", 0), ("notes", 1)],
        "custom positions are dense and follow layout order"
    );

    // 2. What an old cached embed bundle sees is a renderable form.
    let public = service::public_dto_from_model(created.clone()).unwrap();
    assert!(public.standard_fields.email.enabled);
    assert_eq!(public.custom_fields.len(), 2);
    // Current renderers get the real layout: same elements, same order.
    // Positions are renumbered densely on write, so compare structurally.
    let shape = |els: &[FormElement]| -> Vec<String> {
        els.iter()
            .map(|e| match e {
                FormElement::Standard(s) => format!("standard:{}", s.key),
                FormElement::Custom(c) => format!("custom:{}", c.key),
                FormElement::Heading(h) => format!("heading:{}", h.text),
                FormElement::Paragraph(p) => format!("paragraph:{}", p.text),
                FormElement::Divider => "divider".into(),
                FormElement::PageBreak(b) => {
                    format!("page_break:{}", b.title.clone().unwrap_or_default())
                }
            })
            .collect()
    };
    assert_eq!(shape(&public.layout), shape(&layout));
    // The stored layout's custom positions agree with the projection.
    let stored_positions: Vec<i32> = public
        .layout
        .iter()
        .filter_map(|e| match e {
            FormElement::Custom(c) => Some(c.position),
            _ => None,
        })
        .collect();
    assert_eq!(stored_positions, vec![0, 1], "positions renumber from layout order");

    // 3. A legacy-only PATCH must not flatten the hand-ordered layout.
    let mut legacy_sf = public.standard_fields.clone();
    legacy_sf.phone.enabled = true;
    let updated = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            standard_fields: Some(legacy_sf),
            ..Default::default()
        },
    )
    .await
    .expect("legacy patch");

    let after = service::layout_from_model(&updated).unwrap();
    let keys: Vec<&str> = after.iter().filter_map(|e| e.field_key()).collect();
    assert_eq!(
        keys,
        vec!["first_name", "how_heard", "email", "phone", "country", "notes"],
        "interleaving survives, and the new standard lands at its catalogue position"
    );
    assert!(
        after.iter().any(|e| matches!(e, FormElement::Heading(_))),
        "decoration survives a legacy write"
    );
    assert!(after.iter().any(|e| matches!(e, FormElement::PageBreak(_))));

    // 4. Sending both shapes at once is rejected outright.
    let conflict = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            layout: Some(layout.clone()),
            custom_fields: Some(vec![]),
            ..Default::default()
        },
    )
    .await;
    assert!(conflict.is_err(), "layout + legacy pair must be a 400");

    // KEEP_FIXTURE leaves the row behind so the old-embed-bundle compat check
    // has a form that was written through the layout path to point at.
    if std::env::var("KEEP_FIXTURE").is_ok() {
        eprintln!("KEEP_FIXTURE: form id={} slug={}", created.id, slug);
    } else {
        service::delete_form(&db, created.id).await.expect("cleanup");
    }
}
