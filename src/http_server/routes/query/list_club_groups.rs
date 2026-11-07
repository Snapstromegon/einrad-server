use std::collections::HashMap;

use axum::{Extension, Json, extract::Query};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    act_type::ActType,
    groups::{ensure_club_access, group_name_sort_key, type_conflict_message},
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::ReloadableSqlite,
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ClubGroup {
    id: Uuid,
    group_name: String,
    act_type: ActType,
    /// `null` for incomplete groups (fewer than 3 regular riders) and groups
    /// with a `conflict`.
    category: Option<String>,
    /// Age in whole years of the oldest rider (replacements included) on the
    /// determining day.
    max_age: Option<f64>,
    /// Set if a rider of the group is also in another group of the same type:
    /// a German message naming them. The group has no category until that's
    /// sorted out.
    conflict: Option<String>,
    /// The act's title.
    name: String,
    song_file: Option<String>,
    owner_club_id: Option<Uuid>,
    owner_club_name: Option<String>,
    /// Whether the queried club created the group (and may rename or delete it).
    is_owner: bool,
    /// Regular riders first, then replacements, each sorted by name.
    members: Vec<ClubGroupMember>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ClubGroupMember {
    rider_id: Uuid,
    firstname: String,
    lastname: String,
    club_id: Uuid,
    club_name: String,
    is_replacement: bool,
    /// Only for the queried club's own riders.
    #[serde(with = "time::serde::iso8601::option")]
    birthdate: Option<time::OffsetDateTime>,
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListClubGroupsQuery {
    club_id: Uuid,
}

/// List the groups of a club.
///
/// Groups the club created or has riders in, with all their members.
#[utoipa::path(
    get,
    tags=["query", "club", "group"],
    params(ListClubGroupsQuery),
    path="/list_club_groups",
    responses(
        (status=200, content_type="application/json", body=Vec<ClubGroup>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_club_groups(
    Extension(db): Extension<ReloadableSqlite>,
    Query(query): Query<ListClubGroupsQuery>,
    auth: Auth,
) -> Result<Json<Vec<ClubGroup>>, HttpError> {
    let db = db.get().await.clone();
    let mut connection = db.acquire().await?;
    ensure_club_access(&mut connection, &auth, query.club_id).await?;

    let groups = sqlx::query!(
        r#"
        SELECT
            view_act.id as "id!: Uuid",
            view_act.group_name as "group_name!",
            view_act.act_type as "act_type!: ActType",
            view_act.category,
            view_act.max_age as "max_age: f64",
            view_act.name,
            view_act.song_file,
            view_act.owner_club_id as "owner_club_id: Uuid",
            owner.name as "owner_club_name?"
        FROM view_act
        LEFT JOIN clubs AS owner ON owner.id = view_act.owner_club_id
        WHERE view_act.group_name IS NOT NULL
            AND (
                view_act.owner_club_id = ?1
                OR EXISTS (
                    SELECT 1 FROM act_participants
                    JOIN rider ON rider.id = act_participants.rider_id
                    WHERE act_participants.act_id = view_act.id AND rider.club_id = ?1
                )
            )
        "#,
        query.club_id
    )
    .fetch_all(&mut *connection)
    .await?;

    // All members of these groups in one query, grouped by act below.
    let members = sqlx::query!(
        r#"
        SELECT
            act_participants.act_id as "act_id!: Uuid",
            rider.id as "rider_id!: Uuid",
            rider.firstname,
            rider.lastname,
            rider.club_id as "club_id!: Uuid",
            clubs.name as club_name,
            act_participants.is_replacement,
            rider.birthdate
        FROM act_participants
        JOIN acts ON acts.id = act_participants.act_id
        JOIN rider ON rider.id = act_participants.rider_id
        JOIN clubs ON clubs.id = rider.club_id
        WHERE acts.group_name IS NOT NULL
            AND (
                acts.owner_club_id = ?1
                OR EXISTS (
                    SELECT 1 FROM act_participants own
                    JOIN rider own_rider ON own_rider.id = own.rider_id
                    WHERE own.act_id = acts.id AND own_rider.club_id = ?1
                )
            )
        ORDER BY act_participants.is_replacement, rider.lastname, rider.firstname
        "#,
        query.club_id
    )
    .fetch_all(&mut *connection)
    .await?;

    let mut members_by_group: HashMap<Uuid, Vec<ClubGroupMember>> = HashMap::new();
    for member in members {
        members_by_group
            .entry(member.act_id)
            .or_default()
            .push(ClubGroupMember {
                rider_id: member.rider_id,
                firstname: member.firstname,
                lastname: member.lastname,
                club_id: member.club_id,
                club_name: member.club_name,
                is_replacement: member.is_replacement,
                birthdate: (member.club_id == query.club_id).then_some(member.birthdate),
            });
    }

    let mut club_groups = Vec::with_capacity(groups.len());
    for group in groups {
        let conflict = type_conflict_message(&mut connection, group.id).await?;
        club_groups.push(ClubGroup {
            id: group.id,
            group_name: group.group_name,
            act_type: group.act_type,
            category: group.category,
            // Whole years are enough to tell U15 from 15+; the exact age
            // would give away the birthdate of other clubs' riders.
            max_age: group.max_age.map(f64::floor),
            conflict,
            name: group.name,
            song_file: group.song_file,
            owner_club_id: group.owner_club_id,
            owner_club_name: group.owner_club_name,
            is_owner: group.owner_club_id == Some(query.club_id),
            members: members_by_group.remove(&group.id).unwrap_or_default(),
        });
    }
    // Sorted in Rust, because SQLite's NOCASE only folds ASCII letters.
    club_groups.sort_by_cached_key(|group| group_name_sort_key(&group.group_name));
    Ok(Json(club_groups))
}
