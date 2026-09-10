//! Read-after-write check for the form `display_name` column.
//!
//! The hazard is the fallback. `display_name` is nullable and `NULL` means "use
//! `name`", so "never set one" and "set one, then cleared it" must be the same
//! row — the same no-backfill stance `layout` and `post_submission_action`
//! take. What is unique here is *where* the fallback resolves: server-side,
//! into `PublicFormDto.name`, so an embed bundle cached on a third-party host
//! page honours a display name without an upgrade (it is just reading the
//! `name` it always read). That is why the renderer has no `display_name`
//! concept at all, and this test pins it down so nobody "fixes" it by adding
//! one.
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test display_name -- --ignored --nocapture
//! ```

use open_relay_core::backend::BackendRegistry;
use open_relay_core::error::CoreError;
use open_relay_core::forms::{NewForm, UpdateForm, service};
use sea_orm::Database;

fn registry() -> BackendRegistry {
    let mut r = BackendRegistry::new();
    r.register_static(std::sync::Arc::new(
        open_relay_core::backend::openrelay::OpenRelayBackend,
    ));
    r
}

const ADMIN_NAME: &str = "Display name integration";

fn new_form(slug: &str, display_name: Option<&str>) -> NewForm {
    NewForm {
        name: ADMIN_NAME.into(),
        display_name: display_name.map(str::to_string),
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
        metadata: None,
    }
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn display_name_survives_a_round_trip() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("display-name-it-{}", std::process::id());

    // 1. No display name stores NULL, so an untouched form is byte-identical on
    //    disk to a row written before the column existed — and the visitor sees
    //    the admin's own name, exactly as before.
    let created = service::create_form(&db, &reg, 1, new_form(&slug, None))
        .await
        .expect("create");
    assert!(
        created.display_name.is_none(),
        "an unset display name must be stored as NULL, not as an empty string"
    );
    assert_eq!(
        service::public_dto_from_model(created.clone())
            .unwrap()
            .name,
        ADMIN_NAME,
        "with no display name the public title falls back to `name`"
    );

    // 2. A display name replaces the *public* title only. The admin read path
    //    keeps reporting the internal name, with the raw column beside it.
    //    This pair of assertions is what the whole design rests on.
    let want = "Book a demo";
    let updated = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            display_name: Some(want.into()),
            ..Default::default()
        },
    )
    .await
    .expect("set a display name");
    assert_eq!(updated.display_name.as_deref(), Some(want));
    assert_eq!(
        service::public_dto_from_model(updated.clone())
            .unwrap()
            .name,
        want,
        "public read path — this is what the embed bundle sees as the heading"
    );
    let admin = service::dto_from_model(&db, updated).await.unwrap();
    assert_eq!(
        admin.name, ADMIN_NAME,
        "admin read path must keep the internal name, not the display name"
    );
    assert_eq!(
        admin.display_name.as_deref(),
        Some(want),
        "admins see the raw column so the editor can render a blank field"
    );

    // 3. Surrounding whitespace is trimmed rather than stored.
    let padded = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            display_name: Some("  Talk to sales  ".into()),
            ..Default::default()
        },
    )
    .await
    .expect("set a padded display name");
    assert_eq!(padded.display_name.as_deref(), Some("Talk to sales"));

    // 4. An unrelated PATCH leaves the display name alone.
    let renamed = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            name: Some("Renamed".into()),
            ..Default::default()
        },
    )
    .await
    .expect("rename");
    assert_eq!(
        renamed.display_name.as_deref(),
        Some("Talk to sales"),
        "an unrelated PATCH must not reset the display name"
    );

    // 5. A blank string clears the column back to NULL — that is how the admin
    //    reverts — and the public title falls back to the (now renamed) name.
    let cleared = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            display_name: Some("   ".into()),
            ..Default::default()
        },
    )
    .await
    .expect("clear the display name");
    assert!(
        cleared.display_name.is_none(),
        "a blank display name must clear the column, not store whitespace"
    );
    assert_eq!(
        service::public_dto_from_model(cleared).unwrap().name,
        "Renamed",
        "cleared falls back to `name` again, indistinguishable from never set"
    );

    // 6. Oversize input is a 400 rather than a silent truncation.
    let err = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            display_name: Some("x".repeat(1000)),
            ..Default::default()
        },
    )
    .await
    .expect_err("an oversize display name must be rejected");
    assert!(
        matches!(err, CoreError::BadRequest(_)),
        "expected a bad request, got {err:?}"
    );

    service::delete_form(&db, created.id)
        .await
        .expect("cleanup");
}
