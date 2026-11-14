use axum::{Extension, Json};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    act_type::ActType,
    groups::group_name_sort_key,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::ReloadableSqlite,
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct GroupSummary {
    id: Uuid,
    group_name: String,
    act_type: ActType,
    /// `null` for incomplete groups (fewer than 3 regular riders).
    category_id: Option<Uuid>,
    category: Option<String>,
    owner_club_id: Option<Uuid>,
    owner_club_name: Option<String>,
    regular_count: i64,
    replacement_count: i64,
    /// The clubs with riders in the group, sorted by name.
    club_names: Vec<String>,
}

/// List all groups.
///
/// Lets clubs pick an existing group to join. Contains no rider data.
#[utoipa::path(
    get,
    tags=["query", "group"],
    path="/list_groups",
    responses(
        (status=200, content_type="application/json", body=Vec<GroupSummary>),
        (status=401, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_groups(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
) -> Result<Json<Vec<GroupSummary>>, HttpError> {
    let db = db.get().await.clone();
    let groups = sqlx::query!(
        r#"
        SELECT
            view_act.id as "id!: Uuid",
            view_act.group_name as "group_name!",
            view_act.act_type as "act_type!: ActType",
            CASE WHEN view_act.withdrawn_at IS NULL THEN view_act.category_id END as "category_id?: Uuid",
            CAST(CASE WHEN view_act.withdrawn_at IS NULL THEN view_act.category END AS TEXT) as "category?: String",
            view_act.owner_club_id as "owner_club_id: Uuid",
            owner.name as "owner_club_name?",
            view_act.regular_count as "regular_count!: i64",
            view_act.replacement_count as "replacement_count!: i64",
            (
                SELECT json_group_array(name) FROM (
                    SELECT DISTINCT clubs.name FROM act_participants
                    JOIN rider ON rider.id = act_participants.rider_id
                    JOIN clubs ON clubs.id = rider.club_id
                    WHERE act_participants.act_id = view_act.id
                    ORDER BY clubs.name
                )
            ) as "club_names!: sqlx::types::Json<Vec<String>>"
        FROM view_act
        LEFT JOIN clubs AS owner ON owner.id = view_act.owner_club_id
        WHERE view_act.group_name IS NOT NULL
        "#
    )
    .fetch_all(&db)
    .await?;

    let mut groups: Vec<GroupSummary> = groups
        .into_iter()
        .map(|group| GroupSummary {
            id: group.id,
            group_name: group.group_name,
            act_type: group.act_type,
            category_id: group.category_id,
            category: group.category,
            owner_club_id: group.owner_club_id,
            owner_club_name: group.owner_club_name,
            regular_count: group.regular_count,
            replacement_count: group.replacement_count,
            club_names: group.club_names.0,
        })
        .collect();
    // Sorted in Rust, because SQLite's NOCASE only folds ASCII letters.
    groups.sort_by_cached_key(|group| group_name_sort_key(&group.group_name));
    Ok(Json(groups))
}
