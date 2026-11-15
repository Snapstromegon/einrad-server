//! `GroupQueries` for SQLite: groups of several riders. One read transaction
//! per query.

use std::collections::HashMap;

use db_core::{
    act_type::ActType,
    actor::Actor,
    error::QueryError,
    groups::group_name_sort_key,
    ids::{ActId, CategoryId, ClubId, RiderId},
    queries::group::{ClubGroup, ClubGroupMember, GroupQueries, GroupSummary, ListClubGroups},
};
use sqlx::SqliteConnection;

use super::{SqliteRepo, read_tx};
use crate::{group_rules::type_conflict_message, registration::club_access};

#[async_trait::async_trait]
impl GroupQueries for SqliteRepo {
    async fn list_club_groups(
        &self,
        actor: &Actor,
        query: ListClubGroups,
    ) -> Result<Vec<ClubGroup>, QueryError> {
        read_tx!(self, tx, {
            club_access(&mut tx, actor, query.club_id).await?;
            Ok(list_club_groups(&mut tx, query.club_id).await?)
        })
    }

    async fn list_groups(&self, _actor: &Actor) -> Result<Vec<GroupSummary>, QueryError> {
        read_tx!(self, tx, { Ok(list_groups(&mut tx).await?) })
    }
}

async fn list_club_groups(
    conn: &mut SqliteConnection,
    club: ClubId,
) -> sqlx::Result<Vec<ClubGroup>> {
    let groups = sqlx::query!(
            r#"
        SELECT
            view_act.id as "id!: ActId",
            view_act.group_name as "group_name!",
            view_act.act_type as "act_type!: ActType",
            CASE WHEN view_act.withdrawn_at IS NULL THEN view_act.category_id END as "category_id?: CategoryId",
            CAST(CASE WHEN view_act.withdrawn_at IS NULL THEN view_act.category END AS TEXT) as "category?: String",
            view_act.max_age as "max_age: f64",
            view_act.name,
            view_act.song_file,
            view_act.owner_club_id as "owner_club_id: ClubId",
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
            club
        )
        .fetch_all(&mut *conn)
        .await?;

    // All members of these groups in one query, grouped by act below.
    let members = sqlx::query!(
        r#"
        SELECT
            act_participants.act_id as "act_id!: ActId",
            rider.id as "rider_id!: RiderId",
            rider.firstname,
            rider.lastname,
            rider.club_id as "club_id!: ClubId",
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
        club
    )
    .fetch_all(&mut *conn)
    .await?;

    let mut members_by_group: HashMap<ActId, Vec<ClubGroupMember>> = HashMap::new();
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
                birthdate: (member.club_id == club).then_some(member.birthdate),
            });
    }

    let mut club_groups = Vec::with_capacity(groups.len());
    for group in groups {
        let conflict = type_conflict_message(conn, group.id).await?;
        club_groups.push(ClubGroup {
            id: group.id,
            group_name: group.group_name,
            act_type: group.act_type,
            category_id: group.category_id,
            category: group.category,
            // Whole years are enough to tell U15 from 15+; the exact age
            // would give away the birthdate of other clubs' riders.
            max_age: group.max_age.map(f64::floor),
            conflict,
            name: group.name,
            song_file: group.song_file,
            owner_club_id: group.owner_club_id,
            owner_club_name: group.owner_club_name,
            is_owner: group.owner_club_id == Some(club),
            members: members_by_group.remove(&group.id).unwrap_or_default(),
        });
    }
    // Sorted in Rust, because SQLite's NOCASE only folds ASCII letters.
    club_groups.sort_by_cached_key(|group| group_name_sort_key(&group.group_name));
    Ok(club_groups)
}

async fn list_groups(conn: &mut SqliteConnection) -> sqlx::Result<Vec<GroupSummary>> {
    let groups = sqlx::query!(
            r#"
        SELECT
            view_act.id as "id!: ActId",
            view_act.group_name as "group_name!",
            view_act.act_type as "act_type!: ActType",
            CASE WHEN view_act.withdrawn_at IS NULL THEN view_act.category_id END as "category_id?: CategoryId",
            CAST(CASE WHEN view_act.withdrawn_at IS NULL THEN view_act.category END AS TEXT) as "category?: String",
            view_act.owner_club_id as "owner_club_id: ClubId",
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
        .fetch_all(&mut *conn)
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
    Ok(groups)
}
