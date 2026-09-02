//! Read-after-write check for the form `progress_indicator` column.
//!
//! The hazard mirrors `post_submission_action`: the column is stored as `NULL`
//! when it holds the default, so "never configured" and "configured back to the
//! default" must read back identically. The wrinkle unique to this column is
//! that its default is *not* the pre-column behaviour — forms written before it
//! drew a `Step N of M` line, and a `NULL` now decodes to the bar. That is
//! deliberate, and this test pins it down so nobody "fixes" it into a backfill.
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test progress_indicator -- --ignored --nocapture
//! ```

use open_relay_core::backend::BackendRegistry;
use open_relay_core::forms::{NewForm, ProgressIndicator, ProgressStyle, UpdateForm, service};
use sea_orm::Database;

fn registry() -> BackendRegistry {
    let mut r = BackendRegistry::new();
    r.register_static(std::sync::Arc::new(
        open_relay_core::backend::openrelay::OpenRelayBackend,
    ));
    r
}

fn new_form(slug: &str, indicator: ProgressIndicator) -> NewForm {
    NewForm {
        name: "Progress indicator integration".into(),
        slug: Some(slug.into()),
        standard_fields: None,
        custom_fields: vec![],
        layout: None,
        backends: None,
        tags: vec![],
        reps: vec![],
        source_params: vec![],
        post_submission_action: Default::default(),
        progress_indicator: indicator,
        metadata: None,
    }
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn progress_indicator_survives_a_round_trip() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("progress-it-{}", std::process::id());

    // 1. The default stores NULL, so an untouched form is byte-identical on
    //    disk to a row written before the column existed.
    let created = service::create_form(&db, &reg, 1, new_form(&slug, Default::default()))
        .await
        .expect("create");
    assert!(
        created.progress_indicator.is_none(),
        "the default indicator must be stored as NULL, not as a JSON body"
    );
    assert_eq!(
        service::progress_indicator_from_model(&created).unwrap(),
        ProgressIndicator {
            style: ProgressStyle::Bar,
            show_percent: true
        },
        "a NULL column decodes to the bar — deliberately not the pre-column text row"
    );

    // 2. A non-default indicator round-trips through both read paths.
    let want = ProgressIndicator {
        style: ProgressStyle::Steps,
        show_percent: false,
    };
    let updated = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            progress_indicator: Some(want),
            ..Default::default()
        },
    )
    .await
    .expect("update to the step text");
    assert!(updated.progress_indicator.is_some());
    assert_eq!(
        service::public_dto_from_model(updated.clone())
            .unwrap()
            .progress_indicator,
        want,
        "public read path — this is what the embed bundle sees"
    );
    assert_eq!(
        service::dto_from_model(&db, updated)
            .await
            .unwrap()
            .progress_indicator,
        want,
        "admin read path"
    );

    // 3. `none` is a real stored value, distinct from the NULL default.
    let hidden = ProgressIndicator {
        style: ProgressStyle::None,
        show_percent: true,
    };
    let updated = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            progress_indicator: Some(hidden),
            ..Default::default()
        },
    )
    .await
    .expect("update to none");
    assert!(
        updated.progress_indicator.is_some(),
        "`none` must not collapse to NULL"
    );
    assert_eq!(
        service::progress_indicator_from_model(&updated).unwrap(),
        hidden
    );

    // 4. An unrelated PATCH leaves the indicator alone.
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
        service::progress_indicator_from_model(&renamed).unwrap(),
        hidden,
        "an unrelated PATCH must not reset the indicator"
    );

    // 5. Setting the default back clears the column to NULL again, so a form
    //    reverted in the admin is indistinguishable from one never configured.
    let reverted = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            progress_indicator: Some(ProgressIndicator::default()),
            ..Default::default()
        },
    )
    .await
    .expect("revert to default");
    assert!(
        reverted.progress_indicator.is_none(),
        "reverting to the default must clear the column, not store an equivalent body"
    );

    service::delete_form(&db, created.id)
        .await
        .expect("cleanup");
}
