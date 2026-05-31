//! Read-only aggregation behind the dashboard overview.
//!
//! Everything is a small fixed set of queries — four counts, two grouped
//! roll-ups, and one bounded recent-rows fetch — so the whole payload is cheap
//! to assemble in a single request. Form names are resolved by batch-fetching
//! the referenced `form` rows into an id→name map rather than SQL joins, which
//! keeps the queries simple and the N bounded by the page size.

use std::collections::HashMap;

use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect,
};

use super::{
    DashboardOverview, DashboardTotals, DeliveryStatusCount, FormSubmissionCount, RecentSubmission,
};
use crate::error::CoreResult;

const TOP_FORMS_LIMIT: u64 = 5;
const RECENT_SUBMISSIONS_LIMIT: u64 = 10;

/// Row shape for the `GROUP BY status` delivery roll-up.
#[derive(Debug, FromQueryResult)]
struct StatusGroup {
    status: String,
    count: u64,
}

/// Row shape for the `GROUP BY form_id` submission roll-up.
#[derive(Debug, FromQueryResult)]
struct FormGroup {
    form_id: i32,
    count: u64,
}

/// Assemble the dashboard payload. When `include_recent` is false the
/// `recent_submissions` field is left `None` (the caller lacks
/// `submissions:read`); aggregate counts are returned regardless.
pub async fn overview<C: ConnectionTrait>(
    conn: &C,
    include_recent: bool,
) -> CoreResult<DashboardOverview> {
    let totals = DashboardTotals {
        users: entity::user::Entity::find().count(conn).await?,
        forms: entity::form::Entity::find().count(conn).await?,
        submissions: entity::submission::Entity::find().count(conn).await?,
        backends: entity::backend_instance::Entity::find().count(conn).await?,
    };

    let delivery_status = delivery_status_breakdown(conn).await?;
    let top_forms = top_forms(conn).await?;
    let recent_submissions = if include_recent {
        Some(recent_submissions(conn).await?)
    } else {
        None
    };

    Ok(DashboardOverview {
        totals,
        delivery_status,
        top_forms,
        recent_submissions,
    })
}

/// `SELECT status, COUNT(*) FROM submission_delivery GROUP BY status`.
async fn delivery_status_breakdown<C: ConnectionTrait>(
    conn: &C,
) -> CoreResult<Vec<DeliveryStatusCount>> {
    let groups = entity::submission_delivery::Entity::find()
        .select_only()
        .column(entity::submission_delivery::Column::Status)
        .column_as(entity::submission_delivery::Column::Id.count(), "count")
        .group_by(entity::submission_delivery::Column::Status)
        .into_model::<StatusGroup>()
        .all(conn)
        .await?;

    Ok(groups
        .into_iter()
        .map(|g| DeliveryStatusCount {
            status: g.status,
            count: g.count,
        })
        .collect())
}

/// Top forms by submission volume, names resolved via a single batch fetch.
async fn top_forms<C: ConnectionTrait>(conn: &C) -> CoreResult<Vec<FormSubmissionCount>> {
    let groups = entity::submission::Entity::find()
        .select_only()
        .column(entity::submission::Column::FormId)
        .column_as(entity::submission::Column::Id.count(), "count")
        .group_by(entity::submission::Column::FormId)
        .order_by_desc(entity::submission::Column::Id.count())
        .limit(TOP_FORMS_LIMIT)
        .into_model::<FormGroup>()
        .all(conn)
        .await?;

    let form_ids: Vec<i32> = groups.iter().map(|g| g.form_id).collect();
    let names = form_names(conn, &form_ids).await?;

    Ok(groups
        .into_iter()
        .map(|g| FormSubmissionCount {
            form_name: names
                .get(&g.form_id)
                .cloned()
                .unwrap_or_else(|| format!("Form #{}", g.form_id)),
            form_id: g.form_id,
            count: g.count,
        })
        .collect())
}

/// The most recent submissions, condensed for the activity feed.
async fn recent_submissions<C: ConnectionTrait>(conn: &C) -> CoreResult<Vec<RecentSubmission>> {
    let rows = entity::submission::Entity::find()
        .order_by_desc(entity::submission::Column::CreatedAt)
        .limit(RECENT_SUBMISSIONS_LIMIT)
        .all(conn)
        .await?;

    let form_ids: Vec<i32> = rows.iter().map(|s| s.form_id).collect();
    let names = form_names(conn, &form_ids).await?;

    Ok(rows
        .into_iter()
        .map(|s| RecentSubmission {
            form_name: names.get(&s.form_id).cloned(),
            name: join_name(s.first_name.as_deref(), s.last_name.as_deref()),
            email: s.email,
            id: s.id,
            form_id: s.form_id,
            created_at: s.created_at,
        })
        .collect())
}

/// Batch-fetch `id → name` for the given form ids. Tolerates duplicate/empty
/// input; missing ids simply don't appear in the map.
async fn form_names<C: ConnectionTrait>(
    conn: &C,
    form_ids: &[i32],
) -> CoreResult<HashMap<i32, String>> {
    if form_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let forms = entity::form::Entity::find()
        .filter(entity::form::Column::Id.is_in(form_ids.iter().copied()))
        .all(conn)
        .await?;
    Ok(forms.into_iter().map(|f| (f.id, f.name)).collect())
}

/// Combine first/last name into a single display string, ignoring blanks.
fn join_name(first: Option<&str>, last: Option<&str>) -> Option<String> {
    let joined = [first, last]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}
