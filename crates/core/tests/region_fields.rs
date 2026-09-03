//! Read-after-write check for country and subdivision pickers, end to end.
//!
//! Four things about this feature only break in ways a live database can
//! reveal, so the unit tests over the pure functions aren't enough:
//!
//! 1. A state picker's `country_field` lives inside the `layout` JSON column
//!    and rides along in `custom_fields` too (a `Custom` element's config is
//!    the `CustomField` JSON verbatim). Both halves have to survive a real
//!    write and read back.
//! 2. `create_submission` reads the layout off the row, and a subdivision is
//!    validated against the *country answered in the same payload*. That the
//!    cross-field check reaches the stored row — and therefore the backend
//!    payload — is the whole point.
//! 3. Two independent pairs must not contaminate each other. That is the
//!    reason these are custom types rather than more standard fields.
//! 4. A legacy-only PATCH has no vocabulary for a country reference but moves
//!    elements around, so it must repair rather than 400.
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test region_fields -- --ignored --nocapture
//! ```

use open_relay_core::backend::BackendRegistry;
use open_relay_core::forms::{
    CustomField, CustomFieldType, FieldWidth, FormElement, NewForm, StandardElement,
    StandardFieldConfig, StandardFieldsConfig, UpdateForm, service,
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
        name: "Region fields integration".into(),
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

fn region(key: &str, kind: CustomFieldType, required: bool) -> FormElement {
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
        visible_when: None,
    })
}

fn country(key: &str) -> FormElement {
    region(key, CustomFieldType::Country, true)
}

fn state(key: &str, parent: &str) -> FormElement {
    region(
        key,
        CustomFieldType::State { country_field: Some(parent.into()) },
        true,
    )
}

fn payload(pairs: &[(&str, JsonValue)]) -> NewSubmissionPayload {
    NewSubmissionPayload(
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect(),
    )
}

fn kind_of(layout: &[FormElement], key: &str) -> CustomFieldType {
    layout
        .iter()
        .find_map(|el| match el {
            FormElement::Custom(c) if c.key == key => Some(c.kind.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no custom field {key}"))
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn region_fields_survive_a_round_trip_and_validate_on_submit() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("region-it-{}", std::process::id());

    // Two independent pairs — the shape the standard singleton pair cannot express.
    let layout = vec![
        FormElement::Standard(StandardElement {
            required: true,
            ..StandardElement::from_legacy("email", &StandardFieldConfig::default_enabled())
        }),
        country("billing_country"),
        state("billing_state", "billing_country"),
        country("shipping_country"),
        state("shipping_state", "shipping_country"),
    ];

    // 1. The reference survives the write and comes back on every read path.
    let created = service::create_form(&db, &reg, 1, new_form(&slug, layout.clone()))
        .await
        .expect("create");
    let stored = service::layout_from_model(&created).expect("parse layout");
    assert_eq!(
        kind_of(&stored, "billing_state"),
        CustomFieldType::State { country_field: Some("billing_country".into()) },
    );

    let admin = service::dto_from_model(&db, created.clone())
        .await
        .expect("admin dto");
    assert_eq!(
        kind_of(&admin.layout, "shipping_state"),
        CustomFieldType::State { country_field: Some("shipping_country".into()) },
    );

    let public = service::public_dto_from_model(created.clone()).expect("public dto");
    // A custom element's config *is* the `CustomField` JSON, so the reference
    // rides along in the legacy column too — unlike a standard element's.
    assert!(
        public
            .custom_fields
            .iter()
            .any(|f| f.key == "billing_state" && f.kind.country_field() == Some("billing_country")),
        "the reference rides along in the legacy column"
    );
    // This form needs the subdivision table, so the response carries it.
    let packed = public.regions.expect("a form with a state picker carries regions");
    assert!(packed.contains("US=") && packed.contains("~CA="));

    // 2. A subdivision is checked against the country answered beside it, and
    //    the two pairs don't contaminate each other.
    let ok = submissions::create_submission(
        &db,
        &created,
        payload(&[
            ("email", json!("a@b.co")),
            ("billing_country", json!("US")),
            ("billing_state", json!("CA")),
            ("shipping_country", json!("CA")),
            ("shipping_state", json!("BC")),
        ]),
    )
    .await
    .expect("a valid pair of pairs");
    let custom = ok.custom_data.clone();
    assert_eq!(custom["billing_state"], json!("CA"), "the bare code, not US-CA");
    assert_eq!(custom["shipping_state"], json!("BC"));
    assert_eq!(custom["billing_country"], json!("US"));

    // British Columbia is not a US state, even though California is.
    let crossed = submissions::create_submission(
        &db,
        &created,
        payload(&[
            ("email", json!("a@b.co")),
            ("billing_country", json!("US")),
            ("billing_state", json!("BC")),
            ("shipping_country", json!("CA")),
            ("shipping_state", json!("BC")),
        ]),
    )
    .await;
    assert!(crossed.is_err(), "a subdivision of the wrong country is rejected");

    // 3. A legacy-only PATCH cannot see the reference, so it must repair, not
    //    400. Disabling every standard field leaves the customs in place.
    let patched = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            standard_fields: Some(StandardFieldsConfig::all_disabled()),
            ..Default::default()
        },
    )
    .await
    .expect("a legacy PATCH must never 400 on a reference it can't see");
    let after = service::layout_from_model(&patched).expect("parse layout");
    assert_eq!(
        kind_of(&after, "billing_state"),
        CustomFieldType::State { country_field: Some("billing_country".into()) },
        "the custom country is still ahead of it, so the reference stands"
    );
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn a_form_without_a_state_picker_is_not_sent_the_subdivision_table() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("region-none-it-{}", std::process::id());

    // A country picker on its own needs nothing extra — the country list is in
    // the bundle. This is what keeps the ~40 KB table off almost every page.
    let created = service::create_form(
        &db,
        &reg,
        1,
        new_form(&slug, vec![country("billing_country")]),
    )
    .await
    .expect("create");
    let public = service::public_dto_from_model(created).expect("public dto");
    assert!(public.regions.is_none());
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn a_legacy_reorder_that_strands_a_reference_unbinds_it() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("region-strand-it-{}", std::process::id());

    let created = service::create_form(
        &db,
        &reg,
        1,
        new_form(&slug, vec![country("c"), state("s", "c")]),
    )
    .await
    .expect("create");

    // A legacy client reorders by `position`, putting the state first. It has
    // no way to know that breaks the reference.
    let mut customs = service::public_dto_from_model(created.clone())
        .expect("public dto")
        .custom_fields;
    customs.sort_by_key(|f| if f.key == "s" { 0 } else { 1 });
    for (i, f) in customs.iter_mut().enumerate() {
        f.position = i as i32;
    }

    let patched = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm { custom_fields: Some(customs), ..Default::default() },
    )
    .await
    .expect("repair, never reject");

    let after = service::layout_from_model(&patched).expect("parse layout");
    assert_eq!(
        kind_of(&after, "s"),
        CustomFieldType::State { country_field: None },
        "stranded by the reorder, so it degrades to free text"
    );

    // And the repaired form still accepts a submission — free text now.
    let ok = submissions::create_submission(
        &db,
        &patched,
        payload(&[("s", json!("Anywhere")), ("c", json!("US"))]),
    )
    .await
    .expect("an unbound state takes free text");
    assert_eq!(ok.custom_data["s"], json!("Anywhere"));
}
