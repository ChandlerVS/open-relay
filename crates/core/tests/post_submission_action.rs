//! Read-after-write check for the form `post_submission_action` column.
//!
//! Two production hazards here. First, the column is stored as `NULL` when it
//! holds the default, so "never configured" and "configured back to the
//! default" must both read back as the built-in thank-you message — that
//! equivalence is what lets forms written before the column existed keep
//! rendering identically with no backfill. Second, the redirect URL reaches
//! `window.location` on a third-party host page, so a non-`http(s)` scheme has
//! to be rejected at the persistence boundary, not just in the admin UI.
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test post_submission_action -- --ignored --nocapture
//! ```

use open_relay_core::backend::BackendRegistry;
use open_relay_core::forms::{
    MessageAction, NewForm, PostSubmissionAction, RedirectAction, UpdateForm, service,
};
use sea_orm::Database;

fn registry() -> BackendRegistry {
    let mut r = BackendRegistry::new();
    r.register_static(std::sync::Arc::new(
        open_relay_core::backend::openrelay::OpenRelayBackend,
    ));
    r
}

fn new_form(slug: &str, action: PostSubmissionAction) -> NewForm {
    NewForm {
        name: "Post submission integration".into(),
        slug: Some(slug.into()),
        standard_fields: None,
        custom_fields: vec![],
        layout: None,
        backends: None,
        tags: vec![],
        reps: vec![],
        source_params: vec![],
        post_submission_action: action,
        metadata: None,
    }
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn post_submission_action_survives_a_round_trip() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let reg = registry();
    let slug = format!("psa-it-{}", std::process::id());

    // 1. A form created with the default action stores NULL, i.e. it is
    //    byte-identical on disk to a row written before the column existed.
    let created = service::create_form(&db, &reg, 1, new_form(&slug, Default::default()))
        .await
        .expect("create");
    assert!(
        created.post_submission_action.is_none(),
        "the default action must be stored as NULL, not as a JSON body"
    );
    assert_eq!(
        service::post_submission_action_from_model(&created).unwrap(),
        PostSubmissionAction::default(),
    );

    // 2. A configured message round-trips through both read paths, with the
    //    copy trimmed and a blank label normalized away.
    let updated = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            post_submission_action: Some(PostSubmissionAction::Message(MessageAction {
                message: Some("  Thanks!\nWe'll be in touch.  ".into()),
                allow_resubmit: true,
                resubmit_label: Some("   ".into()),
            })),
            ..Default::default()
        },
    )
    .await
    .expect("update to a message");
    assert!(updated.post_submission_action.is_some());

    let expected = PostSubmissionAction::Message(MessageAction {
        message: Some("Thanks!\nWe'll be in touch.".into()),
        allow_resubmit: true,
        resubmit_label: None,
    });
    let public = service::public_dto_from_model(updated.clone()).unwrap();
    assert_eq!(public.post_submission_action, expected, "public read path");
    let admin = service::dto_from_model(&db, updated).await.unwrap();
    assert_eq!(admin.post_submission_action, expected, "admin read path");

    // 3. A redirect round-trips, trimmed.
    let updated = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            post_submission_action: Some(PostSubmissionAction::Redirect(RedirectAction {
                url: " https://example.com/thanks?a=1 ".into(),
            })),
            ..Default::default()
        },
    )
    .await
    .expect("update to a redirect");
    assert_eq!(
        service::public_dto_from_model(updated).unwrap().post_submission_action,
        PostSubmissionAction::Redirect(RedirectAction {
            url: "https://example.com/thanks?a=1".into(),
        }),
    );

    // 4. A dangerous scheme is refused at the persistence boundary, and the
    //    previously stored redirect is left untouched.
    let rejected = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            post_submission_action: Some(PostSubmissionAction::Redirect(RedirectAction {
                url: "javascript:alert(1)".into(),
            })),
            ..Default::default()
        },
    )
    .await;
    assert!(rejected.is_err(), "javascript: URL must be a 400");
    let reloaded = service::find_by_id(&db, created.id).await.unwrap().unwrap();
    assert_eq!(
        service::post_submission_action_from_model(&reloaded).unwrap(),
        PostSubmissionAction::Redirect(RedirectAction {
            url: "https://example.com/thanks?a=1".into(),
        }),
        "a rejected write must not clobber the stored action"
    );

    // 5. Setting the default back clears the column to NULL again, so a form
    //    reverted in the admin is indistinguishable from one never configured.
    let reverted = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            post_submission_action: Some(PostSubmissionAction::default()),
            ..Default::default()
        },
    )
    .await
    .expect("revert to default");
    assert!(
        reverted.post_submission_action.is_none(),
        "reverting to the default must clear the column, not store an empty body"
    );

    // 6. An update that doesn't mention the action leaves it alone.
    service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm {
            post_submission_action: Some(PostSubmissionAction::Redirect(RedirectAction {
                url: "https://example.com/thanks".into(),
            })),
            ..Default::default()
        },
    )
    .await
    .expect("set a redirect");
    let renamed = service::update_form(
        &db,
        &reg,
        created.id,
        UpdateForm { name: Some("Renamed".into()), ..Default::default() },
    )
    .await
    .expect("rename");
    assert_eq!(
        service::post_submission_action_from_model(&renamed).unwrap(),
        PostSubmissionAction::Redirect(RedirectAction {
            url: "https://example.com/thanks".into(),
        }),
        "an unrelated PATCH must not reset the action"
    );

    service::delete_form(&db, created.id).await.expect("cleanup");
}
