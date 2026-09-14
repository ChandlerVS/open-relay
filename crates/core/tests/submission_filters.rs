//! Read-side check for the admin submissions list, CSV export and bulk delete.
//!
//! The hazard is disagreement. `list` and `export_csv` share one query builder
//! (`filtered_select`) precisely so an export over the same params returns the
//! rows the table showed; this pins that, plus the pieces that fail silently
//! rather than loudly: a `%` in a search that isn't escaped matches too much,
//! a `custom_data` search that isn't case-folded matches too little, and a CSV
//! cell that isn't defused is a live formula in an admin's spreadsheet.
//!
//! Needs a live MySQL (`docker compose -f infra/docker-compose.yml up -d mysql`)
//! and is ignored by default so `cargo test` stays hermetic:
//!
//! ```text
//! DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
//!   cargo test -p open-relay-core --test submission_filters -- --ignored --nocapture
//! ```

use chrono::{DateTime, Duration, Utc};
use open_relay_core::backend::BackendRegistry;
use open_relay_core::error::CoreError;
use open_relay_core::forms::{NewForm, service as forms_service};
use open_relay_core::submissions::{ListQuery, SubmissionSort, service};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, Database, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter,
};
use serde_json::json;

fn registry() -> BackendRegistry {
    let mut r = BackendRegistry::new();
    r.register_static(std::sync::Arc::new(
        open_relay_core::backend::openrelay::OpenRelayBackend,
    ));
    r
}

fn new_form(slug: &str) -> NewForm {
    NewForm {
        name: "Submission filters integration".into(),
        display_name: None,
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

struct Seed<'a> {
    first_name: &'a str,
    last_name: &'a str,
    email: &'a str,
    company: &'a str,
    custom: serde_json::Value,
    statuses: &'a [&'a str],
    duplicate: bool,
    sales_rep_id: Option<i32>,
    created_at: DateTime<Utc>,
}

async fn seed(db: &DatabaseConnection, form_id: i32, s: Seed<'_>) -> i32 {
    let row = entity::submission::ActiveModel {
        form_id: ActiveValue::Set(form_id),
        first_name: ActiveValue::Set(Some(s.first_name.into())),
        last_name: ActiveValue::Set(Some(s.last_name.into())),
        email: ActiveValue::Set(Some(s.email.into())),
        company: ActiveValue::Set(Some(s.company.into())),
        custom_data: ActiveValue::Set(s.custom),
        sales_rep_id: ActiveValue::Set(s.sales_rep_id),
        is_duplicate: ActiveValue::Set(Some(s.duplicate)),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert submission");

    // `before_save` stamps `created_at` on insert, so backdate with an update.
    let mut backdate: entity::submission::ActiveModel = row.clone().into();
    backdate.created_at = ActiveValue::Set(s.created_at);
    backdate.update(db).await.expect("backdate submission");

    for status in s.statuses {
        entity::submission_delivery::ActiveModel {
            submission_id: ActiveValue::Set(row.id),
            backend_name: ActiveValue::Set("open-relay".into()),
            backend_instance_id: ActiveValue::Set(None),
            status: ActiveValue::Set((*status).into()),
            attempts: ActiveValue::Set(0),
            // Far in the future so a dev server's worker never leases these.
            next_attempt_at: ActiveValue::Set(Utc::now() + Duration::days(3650)),
            last_error: ActiveValue::Set(None),
            delivered_at: ActiveValue::Set(None),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert delivery");
    }
    row.id
}

async fn ids(db: &DatabaseConnection, q: ListQuery) -> Vec<i32> {
    let list = service::list(db, &q).await.expect("list");
    assert_eq!(
        list.total as usize,
        list.items.len(),
        "every fixture query fits one page, so total must equal the page"
    );
    list.items.iter().map(|s| s.id).collect()
}

#[tokio::test]
#[ignore = "requires a live MySQL"]
async fn filters_export_and_bulk_delete_agree() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = Database::connect(&url).await.expect("connect");
    let slug = format!("submission-filters-it-{}", std::process::id());
    let form = forms_service::create_form(&db, &registry(), 1, new_form(&slug))
        .await
        .expect("create form");
    let now = Utc::now();

    let a = seed(&db, form.id, Seed {
        first_name: "Jane",
        last_name: "Doe",
        email: "jane@acme.test",
        company: "Acme",
        custom: json!({ "favourite_colour": "Teal", "budget": "50% off" }),
        statuses: &["succeeded"],
        duplicate: false,
        sales_rep_id: None,
        created_at: now - Duration::days(3),
    })
    .await;
    let b = seed(&db, form.id, Seed {
        first_name: "John",
        last_name: "Smith",
        email: "john@globex.test",
        company: "Globex",
        custom: json!({ "favourite_colour": "red" }),
        statuses: &["succeeded", "permanent_failure"],
        duplicate: false,
        sales_rep_id: Some(424_242),
        created_at: now - Duration::days(1),
    })
    .await;
    let c = seed(&db, form.id, Seed {
        first_name: "Jane",
        last_name: "Roe",
        email: "roe@initech.test",
        company: "=HYPERLINK(\"http://evil.test\",\"click\")",
        custom: json!({}),
        statuses: &[],
        duplicate: true,
        sales_rep_id: None,
        created_at: now,
    })
    .await;
    let d = seed(&db, form.id, Seed {
        first_name: "Ann",
        last_name: "Lee",
        email: "ann@umbrella.test",
        company: "Umbrella",
        custom: json!({ "budget": "500 off" }),
        statuses: &["exhausted"],
        duplicate: false,
        sales_rep_id: None,
        created_at: now - Duration::days(10),
    })
    .await;

    let base = || ListQuery {
        form_id: Some(form.id),
        limit: Some(200),
        ..Default::default()
    };
    let search = |q: &str| ListQuery {
        q: Some(q.into()),
        ..base()
    };

    // Default order is newest id first.
    assert_eq!(ids(&db, base()).await, vec![d, c, b, a]);
    assert_eq!(
        ids(&db, ListQuery { sort: Some(SubmissionSort::Oldest), ..base() }).await,
        vec![a, b, c, d]
    );

    // Search: every term must match, each term anywhere.
    assert_eq!(ids(&db, search("jane")).await, vec![c, a]);
    assert_eq!(ids(&db, search("  JANE   acme ")).await, vec![a], "terms AND");
    assert_eq!(ids(&db, search("teal")).await, vec![a], "custom_data is case-folded");
    assert_eq!(
        ids(&db, search("50%")).await,
        vec![a],
        "`%` must be literal, or `50%` would also match `500 off`"
    );
    assert_eq!(ids(&db, search(&format!("#{b}"))).await, vec![b], "id match");

    // Delivery status: any delivery in the set.
    let status = |s: &str| ListQuery {
        status: Some(s.into()),
        ..base()
    };
    assert_eq!(ids(&db, status("permanent_failure,exhausted")).await, vec![d, b]);
    assert_eq!(ids(&db, status("succeeded")).await, vec![b, a]);
    assert!(matches!(
        service::list(&db, &status("failed")).await,
        Err(CoreError::BadRequest(_))
    ));

    // Duplicates, rep, dates.
    assert_eq!(
        ids(&db, ListQuery { duplicate: Some(true), ..base() }).await,
        vec![c]
    );
    assert_eq!(
        ids(&db, ListQuery { duplicate: Some(false), ..base() }).await,
        vec![d, b, a]
    );
    assert_eq!(
        ids(&db, ListQuery { sales_rep_id: Some(424_242), ..base() }).await,
        vec![b]
    );
    assert_eq!(
        ids(&db, ListQuery {
            from: Some(now - Duration::days(2)),
            to: Some(now - Duration::hours(12)),
            ..base()
        })
        .await,
        vec![b],
        "`from` inclusive, `to` exclusive"
    );
    assert_eq!(
        ids(&db, ListQuery { from: Some(now - Duration::days(2)), ..base() }).await,
        vec![c, b]
    );

    // Filters combine, and `total` counts the whole match, not the page.
    let paged = service::list(&db, &ListQuery {
        limit: Some(1),
        ..search("jane")
    })
    .await
    .unwrap();
    assert_eq!((paged.total, paged.items.len()), (2, 1));

    // Export: same builder, so the same rows, plus defused cells.
    let filter = base().filter().unwrap();
    let csv = service::export_csv(&db, &filter).await.expect("export");
    assert!(csv.starts_with('\u{feff}'), "BOM so Excel reads UTF-8");
    let lines: Vec<&str> = csv.trim_end_matches("\r\n").split("\r\n").collect();
    assert_eq!(lines.len(), 1 + 4, "header + one line per matching submission");
    let header = lines[0];
    assert!(header.contains(",favourite_colour") && header.contains(",budget"));
    assert!(lines[1].starts_with(&format!("{d},")), "export follows the sort");
    assert!(
        csv.contains("\"'=HYPERLINK(\"\"http://evil.test\"\",\"\"click\"\")\""),
        "a formula cell must be prefixed with `'`"
    );
    let failing = ListQuery {
        status: Some("exhausted".into()),
        ..base()
    };
    let exported = service::export_csv(&db, &failing.filter().unwrap()).await.unwrap();
    assert_eq!(exported.trim_end_matches("\r\n").split("\r\n").count(), 2);

    // Bulk delete removes delivery rows with the submissions.
    let deleted = service::delete_submissions(&db, &[a, b, 999_999_999])
        .await
        .expect("bulk delete");
    assert_eq!(deleted, 2, "unknown ids are not counted");
    assert_eq!(ids(&db, base()).await, vec![d, c]);
    let orphans = entity::submission_delivery::Entity::find()
        .filter(entity::submission_delivery::Column::SubmissionId.is_in([a, b]))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(orphans, 0, "delivery rows must go with their submission");

    forms_service::delete_form(&db, form.id).await.expect("cleanup");
}
