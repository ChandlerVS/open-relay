//! Read-after-write check for row blocks, end to end.
//!
//! A row is a flat `RowStart`/`RowEnd` marker pair rather than a container
//! holding `children` (see `RowStartElement` for why). That choice is what this
//! file exists to hold in place, because the ways it can break are exactly the
//! ways only a real write and read-back reveal:
//!
//! 1. The markers live in the `layout` JSON column and have to survive a round
//!    trip through all three read paths without a `config` key going missing.
//! 2. **The projection.** Fields inside a row are still top-level elements, so
//!    `legacy_from_layout` must still write them to `standard_fields`/
//!    `custom_fields`. If that ever stopped being true, an embed bundle cached
//!    on a third-party page — which cannot be force-upgraded, and which ignores
//!    the markers entirely — would render an empty form.
//! 3. `create_submission` reads the layout off the stored row. A row must not
//!    change what validates or what reaches a backend; it is presentation only.
//! 4. A legacy-only PATCH has no vocabulary for rows but moves elements around,
//!    so it must never insert a field into one and never leave a layout the
//!    server would refuse to re-save.
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test layout_rows -- --ignored --nocapture
//! ```

use open_relay_core::backend::BackendRegistry;
use open_relay_core::forms::{
    CustomField, CustomFieldType, FieldWidth, FormElement, NewForm, RowStartElement,
    StandardElement, StandardFieldConfig, StandardFieldsConfig, StandardInputVariant, UpdateForm,
    service,
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
        name: "Layout rows integration".into(),
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

fn standard(key: &str, width: FieldWidth) -> FormElement {
    FormElement::Standard(StandardElement {
        width,
        ..StandardElement::from_legacy(key, &StandardFieldConfig::default_enabled())
    })
}

fn custom(key: &str, kind: CustomFieldType, width: FieldWidth) -> FormElement {
    FormElement::Custom(CustomField {
        key: key.into(),
        label: format!("Label {key}"),
        kind,
        required: false,
        placeholder: None,
        help_text: None,
        position: 0,
        width,
        default_value: None,
        visible_when: None,
    })
}

fn row_start(label: Option<&str>) -> FormElement {
    FormElement::RowStart(RowStartElement {
        label: label.map(str::to_string),
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

/// A compact rendering of a layout's structure, for order-sensitive asserts.
fn shape(els: &[FormElement]) -> Vec<String> {
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
            FormElement::RowStart(r) => {
                format!("row_start:{}", r.label.clone().unwrap_or_default())
            }
            FormElement::RowEnd => "row_end".into(),
        })
        .collect()
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn rows_survive_a_round_trip_and_stay_invisible_to_submissions() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("row-it-{}", std::process::id());

    // The shape this feature was built for: a full-width street address, then
    // city / state / postal code sharing one line.
    let layout = vec![
        standard("email", FieldWidth::Full),
        standard("address_line_1", FieldWidth::Full),
        row_start(Some("City, state, ZIP")),
        standard("city", FieldWidth::Half),
        standard("state", FieldWidth::Third),
        standard("postal_code", FieldWidth::Third),
        FormElement::RowEnd,
        // A custom field in a row too — it takes the other projection path.
        row_start(None),
        custom("nickname", CustomFieldType::Text, FieldWidth::Half),
        custom("shoe_size", CustomFieldType::Number, FieldWidth::Half),
        FormElement::RowEnd,
    ];

    let created = service::create_form(&db, &reg, 1, new_form(&slug, layout.clone()))
        .await
        .expect("create");

    // 1. The stored layout round-trips, markers, labels and widths intact.
    let model = service::find_by_id(&db, created.id).await.expect("find").expect("still there");
    let stored = service::layout_from_model(&model).expect("parse layout");
    assert_eq!(shape(&stored), shape(&layout), "markers survive the write");
    match &stored[2] {
        FormElement::RowStart(r) => assert_eq!(r.label.as_deref(), Some("City, state, ZIP")),
        other => panic!("expected a row start, got {other:?}"),
    }
    match &stored[3] {
        FormElement::Standard(s) => {
            assert_eq!(s.key, "city");
            assert_eq!(s.width, FieldWidth::Half, "a width inside a row is stored");
        }
        other => panic!("expected the city field, got {other:?}"),
    }
    match &stored[4] {
        FormElement::Standard(s) => assert_eq!(s.width, FieldWidth::Third, "the new third width"),
        other => panic!("expected the state field, got {other:?}"),
    }

    // 2. Both read paths agree with the stored row.
    let dto = service::dto_from_model(&db, model.clone()).await.expect("admin dto");
    let public = service::public_dto_from_model(model.clone()).expect("public dto");
    assert_eq!(shape(&dto.layout), shape(&layout));
    assert_eq!(shape(&public.layout), shape(&layout));

    // 3. THE LOAD-BEARING ONE. An embed bundle cached before rows existed reads
    //    the legacy pair and ignores the markers, so every field in a row has
    //    to be present there or that bundle renders an empty form.
    for key in ["email", "address_line_1", "city", "state", "postal_code"] {
        assert!(
            public.standard_fields.get(key).is_some_and(|c| c.enabled),
            "standard field '{key}' must project out of its row"
        );
    }
    assert_eq!(
        public.custom_fields.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(),
        vec!["nickname", "shoe_size"],
        "custom fields in a row still project, in layout order"
    );
    // Positions renumber densely from layout order, markers not counted.
    assert_eq!(
        public.custom_fields.iter().map(|c| c.position).collect::<Vec<_>>(),
        vec![0, 1]
    );

    // 4. A row is presentation only: the submission path is entirely row-blind.
    let sub = submissions::create_submission(
        &db,
        &model,
        payload(&[
            ("email", json!("someone@example.com")),
            ("address_line_1", json!("1 Test Street")),
            ("city", json!("Portland")),
            ("state", json!("Oregon")),
            ("postal_code", json!("97201")),
            ("nickname", json!("Reid")),
            ("shoe_size", json!("11")),
        ]),
    )
    .await
    .expect("a row must not change what validates");
    assert_eq!(sub.city.as_deref(), Some("Portland"));
    assert_eq!(sub.postal_code.as_deref(), Some("97201"));
    assert_eq!(sub.custom_data["nickname"], json!("Reid"));
    assert_eq!(sub.custom_data["shoe_size"], json!(11.0), "string numbers coerce via f64");

    if std::env::var("KEEP_FIXTURE").is_ok() {
        eprintln!("KEEP_FIXTURE: form id={} slug={}", created.id, slug);
    } else {
        service::delete_form(&db, created.id).await.expect("cleanup");
    }
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn a_broken_row_is_rejected_and_leaves_the_stored_layout_alone() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("row-broken-it-{}", std::process::id());

    let good = vec![
        standard("email", FieldWidth::Full),
        row_start(None),
        standard("city", FieldWidth::Half),
        standard("postal_code", FieldWidth::Half),
        FormElement::RowEnd,
    ];
    let created = service::create_form(&db, &reg, 1, new_form(&slug, good.clone()))
        .await
        .expect("create");

    // Every structural break is a 400, and none of them touch the stored row.
    let broken: Vec<(&str, Vec<FormElement>)> = vec![
        (
            "opened but never closed",
            vec![row_start(None), standard("city", FieldWidth::Full)],
        ),
        (
            "closed but never opened",
            vec![standard("city", FieldWidth::Full), FormElement::RowEnd],
        ),
        (
            "nested",
            vec![
                row_start(None),
                row_start(None),
                standard("city", FieldWidth::Full),
                FormElement::RowEnd,
                FormElement::RowEnd,
            ],
        ),
        (
            "closed twice",
            vec![
                row_start(None),
                standard("city", FieldWidth::Full),
                FormElement::RowEnd,
                FormElement::RowEnd,
            ],
        ),
        (
            "a page break inside a row would split one line across two steps",
            vec![
                row_start(None),
                standard("city", FieldWidth::Full),
                FormElement::PageBreak(Default::default()),
                standard("email", FieldWidth::Full),
                FormElement::RowEnd,
            ],
        ),
        (
            "a divider inside a row has no rendering",
            vec![
                row_start(None),
                standard("city", FieldWidth::Full),
                FormElement::Divider,
                FormElement::RowEnd,
            ],
        ),
        (
            "over the per-row cap",
            vec![
                row_start(None),
                standard("first_name", FieldWidth::Full),
                standard("last_name", FieldWidth::Full),
                standard("email", FieldWidth::Full),
                standard("phone", FieldWidth::Full),
                standard("company", FieldWidth::Full),
                FormElement::RowEnd,
            ],
        ),
    ];

    for (why, layout) in broken {
        let res = service::update_form(
            &db,
            &reg,
            created.id,
            UpdateForm {
                layout: Some(layout),
                ..Default::default()
            },
        )
        .await;
        assert!(res.is_err(), "should have been rejected: {why}");
    }

    let reread = service::find_by_id(&db, created.id).await.expect("find").expect("still there");
    let after = service::layout_from_model(&reread).expect("parse layout");
    assert_eq!(shape(&after), shape(&good), "a rejected write changes nothing");

    // An *empty* row is different in kind: it draws nothing, so it is cleaned
    // up quietly rather than 400'd — the same stance normalize_layout takes on
    // a rule with no conditions.
    let updated = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            layout: Some(vec![
                standard("email", FieldWidth::Full),
                row_start(None),
                FormElement::RowEnd,
            ]),
            ..Default::default()
        },
    )
    .await
    .expect("an empty row is debris, not an error");
    let normalized = service::layout_from_model(&updated).expect("parse layout");
    assert_eq!(shape(&normalized), vec!["standard:email"]);

    // Nested markers with nothing in them are the same kind of debris: the
    // cleanup unwinds them from the inside out before validation ever sees a
    // nesting problem to complain about. Only a row with content in it can be
    // meaningfully nested, and that is the case rejected above.
    let updated = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            layout: Some(vec![
                standard("email", FieldWidth::Full),
                row_start(None),
                row_start(None),
                FormElement::RowEnd,
                FormElement::RowEnd,
            ]),
            ..Default::default()
        },
    )
    .await
    .expect("empty markers unwind rather than tripping the nesting rule");
    let normalized = service::layout_from_model(&updated).expect("parse layout");
    assert_eq!(shape(&normalized), vec!["standard:email"]);

    if std::env::var("KEEP_FIXTURE").is_ok() {
        eprintln!("KEEP_FIXTURE: form id={} slug={}", created.id, slug);
    } else {
        service::delete_form(&db, created.id).await.expect("cleanup");
    }
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn a_legacy_write_never_lands_a_field_inside_a_row() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("row-legacy-it-{}", std::process::id());

    let created = service::create_form(
        &db,
        &reg,
        1,
        new_form(
            &slug,
            vec![
                standard("email", FieldWidth::Full),
                row_start(Some("City and ZIP")),
                standard("city", FieldWidth::Half),
                standard("postal_code", FieldWidth::Half),
                FormElement::RowEnd,
            ],
        ),
    )
    .await
    .expect("create");

    // A legacy client echoes the projected pair back with `state` newly on. It
    // has no vocabulary for rows and cannot see one, and `state` sorts between
    // city and postal_code in the catalogue — so the naive insertion point is
    // *inside* the row.
    let mut sf = StandardFieldsConfig::all_disabled();
    sf.email = StandardFieldConfig::default_enabled();
    sf.city = StandardFieldConfig::default_enabled();
    sf.state = StandardFieldConfig::default_enabled();
    sf.postal_code = StandardFieldConfig::default_enabled();

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
    .expect("a legacy PATCH must never 400 over a row it cannot see");

    let layout = service::layout_from_model(&patched).expect("parse layout");
    assert_eq!(
        shape(&layout),
        vec![
            "standard:email",
            "standard:state",
            "row_start:City and ZIP",
            "standard:city",
            "standard:postal_code",
            "row_end",
        ],
        "the new field lands beside the row, never as a third column of it"
    );
    assert!(
        service::validate_layout(&layout).is_ok(),
        "a legacy PATCH must never leave the layout in a state it can't re-save"
    );

    // Now the other direction: disabling every field in the row empties it, and
    // normalize_layout clears the bare markers away.
    let mut sf = StandardFieldsConfig::all_disabled();
    sf.email = StandardFieldConfig::default_enabled();
    let emptied = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            standard_fields: Some(sf),
            ..Default::default()
        },
    )
    .await
    .expect("emptying a row is not an error");
    let layout = service::layout_from_model(&emptied).expect("parse layout");
    assert_eq!(shape(&layout), vec!["standard:email"]);
    assert!(service::validate_layout(&layout).is_ok());

    if std::env::var("KEEP_FIXTURE").is_ok() {
        eprintln!("KEEP_FIXTURE: form id={} slug={}", created.id, slug);
    } else {
        service::delete_form(&db, created.id).await.expect("cleanup");
    }
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn a_state_picker_inside_a_row_still_resolves_its_country() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("row-region-it-{}", std::process::id());

    // Markers register no field key, so "strictly earlier in the layout" still
    // means what it meant — including across a row boundary.
    let layout = vec![
        custom("ship_country", CustomFieldType::Country, FieldWidth::Full),
        row_start(Some("City and state")),
        custom("ship_city", CustomFieldType::Text, FieldWidth::Half),
        custom(
            "ship_state",
            CustomFieldType::State {
                country_field: Some("ship_country".into()),
            },
            FieldWidth::Half,
        ),
        FormElement::RowEnd,
    ];
    let created = service::create_form(&db, &reg, 1, new_form(&slug, layout.clone()))
        .await
        .expect("create");
    let model = service::find_by_id(&db, created.id).await.expect("find").expect("still there");

    // The subdivision table still ships — needs_subdivisions sees into rows
    // because the field is still a top-level element.
    let public = service::public_dto_from_model(model.clone()).expect("public dto");
    assert!(
        public.regions.is_some(),
        "a state picker in a row still needs the subdivision table"
    );

    // Cross-field coercion is unchanged: the country is coerced first because
    // custom_fields comes out of the layout in order, markers and all.
    let sub = submissions::create_submission(
        &db,
        &model,
        payload(&[
            ("ship_country", json!("US")),
            ("ship_city", json!("Portland")),
            ("ship_state", json!("OR")),
        ]),
    )
    .await
    .expect("submit");
    assert_eq!(sub.custom_data["ship_state"], json!("OR"));

    // And a state that isn't in the named country is still a 400.
    let bad = submissions::create_submission(
        &db,
        &model,
        payload(&[
            ("ship_country", json!("FR")),
            ("ship_city", json!("Paris")),
            ("ship_state", json!("OR")),
        ]),
    )
    .await;
    assert!(bad.is_err(), "a row must not weaken cross-field validation");

    // A row is also transparent to the standard country/state pair's rule that
    // both halves must be dropdowns and the country must come first.
    let standard_pair = vec![
        FormElement::Standard(StandardElement {
            input_override: Some(StandardInputVariant::Select),
            ..StandardElement::from_legacy("country", &StandardFieldConfig::default_enabled())
        }),
        row_start(None),
        FormElement::Standard(StandardElement {
            input_override: Some(StandardInputVariant::Select),
            ..StandardElement::from_legacy("state", &StandardFieldConfig::default_enabled())
        }),
        FormElement::RowEnd,
    ];
    assert!(service::validate_layout(&standard_pair).is_ok());

    if std::env::var("KEEP_FIXTURE").is_ok() {
        eprintln!("KEEP_FIXTURE: form id={} slug={}", created.id, slug);
    } else {
        service::delete_form(&db, created.id).await.expect("cleanup");
    }
}
