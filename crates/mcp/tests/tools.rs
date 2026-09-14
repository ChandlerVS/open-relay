//! End-to-end guard for the MCP layout helpers, against a live database.
//!
//! The property under test is the one the whole design rests on: a helper edits
//! one element, but the write still goes through `service::update_form`, so it
//! gets the full normalize → validate → recompute-legacy-columns pipeline. Two
//! ways that could regress, both silent:
//!
//! * a helper writing the layout column directly would leave
//!   `standard_fields`/`custom_fields` stale, and every embed bundle cached on
//!   a third-party page renders from that pair;
//! * a helper skipping `validate_layout` would let an agent build a form whose
//!   `visible_when` points forward, which the renderer cannot resolve.
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-mcp --test tools -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::sync::Arc;

use open_relay_core::api_keys::ApiActor;
use open_relay_core::backend::BackendRegistry;
use open_relay_core::backend::openrelay::OpenRelayBackend;
use open_relay_core::forms::edit::{self, ElementRef};
use open_relay_core::forms::{
    CustomField, CustomFieldType, FieldWidth, FormElement, NewForm, RowStartElement,
    StandardElement, service,
};
use open_relay_core::permissions::Permission;
use open_relay_mcp::handler::{OpenRelayMcp, require};
use sea_orm::{Database, DatabaseConnection};

fn registry() -> BackendRegistry {
    let mut r = BackendRegistry::new();
    r.register_static(Arc::new(OpenRelayBackend));
    r
}

async fn connect() -> DatabaseConnection {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    Database::connect(&url).await.expect("connect")
}

fn actor(perms: &[Permission]) -> ApiActor {
    ApiActor {
        user_id: 1,
        permissions: perms.iter().copied().collect::<HashSet<_>>(),
    }
}

fn custom(key: &str, kind: CustomFieldType) -> FormElement {
    FormElement::Custom(CustomField {
        key: key.into(),
        label: key.into(),
        kind,
        required: false,
        placeholder: None,
        help_text: None,
        position: 0,
        width: FieldWidth::Full,
        default_value: None,
        visible_when: None,
    })
}

fn standard(key: &str) -> FormElement {
    FormElement::Standard(StandardElement {
        key: key.into(),
        required: false,
        label: None,
        placeholder: None,
        help_text: None,
        width: FieldWidth::Full,
        default_value: None,
        input_override: None,
        visible_when: None,
    })
}

fn shape(layout: &[FormElement]) -> Vec<String> {
    layout
        .iter()
        .map(|el| match el {
            FormElement::Standard(s) => format!("std:{}", s.key),
            FormElement::Custom(c) => format!("cus:{}", c.key),
            FormElement::Heading(_) => "heading".into(),
            FormElement::Paragraph(_) => "paragraph".into(),
            FormElement::RichText(_) => "rich_text".into(),
            FormElement::Divider => "divider".into(),
            FormElement::PageBreak(_) => "page_break".into(),
            FormElement::RowStart(_) => "row_start".into(),
            FormElement::RowEnd => "row_end".into(),
        })
        .collect()
}

/// A form with an address row, so the helpers are exercised against the layout
/// features most likely to break under a positional edit.
async fn seed_form(db: &DatabaseConnection, slug: &str) -> i32 {
    let mut layout = vec![standard("email"), custom("bill_country", CustomFieldType::Country)];
    layout.push(FormElement::RowStart(RowStartElement { label: None }));
    layout.push(custom("bill_city", CustomFieldType::Text));
    layout.push(custom(
        "bill_state",
        CustomFieldType::State {
            country_field: Some("bill_country".into()),
        },
    ));
    layout.push(FormElement::RowEnd);

    let created = service::create_form(
        db,
        &registry(),
        1,
        NewForm {
            name: format!("mcp test {slug}"),
            display_name: None,
            slug: Some(slug.to_string()),
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
        },
    )
    .await
    .expect("create form");
    created.id
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn a_layout_helper_keeps_the_legacy_columns_in_step() {
    let db = connect().await;
    let mcp = OpenRelayMcp::http(db.clone(), registry());
    let slug = format!("mcp-legacy-{}", std::process::id());
    let form_id = seed_form(&db, &slug).await;

    // Edit ONE element. If this wrote the layout column directly rather than
    // going through `update_form`, the legacy pair below would still describe
    // the pre-edit form — and every cached embed bundle renders from that pair.
    let dto = mcp
        .edit_layout(form_id, |layout| {
            edit::add_element(
                layout,
                custom("nickname", CustomFieldType::Text),
                Some(1),
            )
        })
        .await
        .expect("add_element");

    assert_eq!(
        shape(&dto.layout),
        [
            "std:email",
            "cus:nickname",
            "cus:bill_country",
            "row_start",
            "cus:bill_city",
            "cus:bill_state",
            "row_end"
        ]
    );

    let legacy_keys: Vec<&str> = dto.custom_fields.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(
        legacy_keys,
        ["nickname", "bill_country", "bill_city", "bill_state"],
        "the legacy custom_fields projection must be recomputed, in layout order"
    );
    assert!(
        dto.standard_fields.email.enabled,
        "the legacy standard_fields projection must still describe the form"
    );
    // `position` is renumbered on every write; a stale one would desync the
    // legacy renderer's ordering from the layout's.
    let positions: Vec<i32> = dto.custom_fields.iter().map(|c| c.position).collect();
    assert_eq!(positions, [0, 1, 2, 3]);

    service::delete_form(&db, form_id).await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn a_helper_cannot_produce_a_layout_the_api_would_reject() {
    let db = connect().await;
    let mcp = OpenRelayMcp::http(db.clone(), registry());
    let slug = format!("mcp-reject-{}", std::process::id());
    let form_id = seed_form(&db, &slug).await;

    // Moving the row ahead of the country it binds to would leave `state`
    // pointing forwards. `validate_layout` is what catches it — the helper
    // itself knows nothing about country bindings.
    let err = mcp
        .edit_layout(form_id, |layout| {
            edit::move_element(layout, &ElementRef::Index(2), 0)
        })
        .await
        .expect_err("a forward country reference must be rejected");
    let message = format!("{err:?}");
    assert!(
        message.contains("bill_country") && message.contains("earlier"),
        "the error should name the offending binding, got: {message}"
    );

    // A rule pointing forwards is refused for the same reason.
    let err = mcp
        .edit_layout(form_id, |layout| {
            let mut el = custom("early", CustomFieldType::Text);
            if let FormElement::Custom(c) = &mut el {
                c.visible_when = Some(open_relay_core::forms::VisibilityRule {
                    match_mode: Default::default(),
                    conditions: vec![open_relay_core::forms::Condition {
                        field: "bill_city".into(),
                        op: open_relay_core::forms::ConditionOp::IsNotEmpty,
                        value: None,
                    }],
                });
            }
            edit::add_element(layout, el, Some(0))
        })
        .await
        .expect_err("a forward visibility rule must be rejected");
    assert!(format!("{err:?}").contains("earlier"), "{err:?}");

    // The form is untouched by the failed edits — each ran in its own
    // transaction and rolled back.
    let model = service::find_by_id(&db, form_id)
        .await
        .expect("find")
        .expect("form");
    let layout = service::layout_from_model(&model).expect("layout");
    assert_eq!(
        shape(&layout),
        ["std:email", "cus:bill_country", "row_start", "cus:bill_city", "cus:bill_state", "row_end"]
    );

    service::delete_form(&db, form_id).await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn removing_a_row_marker_takes_the_whole_row() {
    let db = connect().await;
    let mcp = OpenRelayMcp::http(db.clone(), registry());
    let slug = format!("mcp-row-{}", std::process::id());
    let form_id = seed_form(&db, &slug).await;

    // Index 5 is the RowEnd. Naming it must remove the pair and its contents —
    // leaving half a pair behind would be a 400 on the next write.
    let dto = mcp
        .edit_layout(form_id, |layout| {
            edit::remove_element(layout, &ElementRef::Index(5))
        })
        .await
        .expect("remove row");
    assert_eq!(shape(&dto.layout), ["std:email", "cus:bill_country"]);
    assert_eq!(
        dto.custom_fields.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(),
        ["bill_country"],
        "the row's fields must leave the legacy projection too"
    );

    service::delete_form(&db, form_id).await.expect("cleanup");
}

/// Permission enforcement is a plain set check, so it needs no database — but
/// it is the gate every tool sits behind, and a key scoped to `forms:read` must
/// not be able to write.
#[test]
fn scopes_gate_the_write_tools() {
    let read_only = actor(&[Permission::FormsRead]);
    assert!(require(&read_only, Permission::FormsRead).is_ok());

    let err = require(&read_only, Permission::FormsWrite).expect_err("write must be refused");
    assert!(
        format!("{err:?}").contains("forms:write"),
        "the error should name the missing permission, got: {err:?}"
    );
    assert!(require(&read_only, Permission::FormsDelete).is_err());

    let full = actor(&[
        Permission::FormsRead,
        Permission::FormsWrite,
        Permission::FormsDelete,
    ]);
    assert!(require(&full, Permission::FormsWrite).is_ok());
    assert!(require(&full, Permission::FormsDelete).is_ok());
}
