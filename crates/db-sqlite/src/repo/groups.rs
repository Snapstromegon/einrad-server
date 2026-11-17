use std::collections::{HashMap, HashSet};

use db_core::{
    DbError, GroupsRepo,
    act_type::ActType,
    clubs::{Actor, RegistrationError},
    groups::{
        ClubGroup, ClubGroupMember, GroupMember, GroupSummary, ensure_group_type,
        group_name_sort_key, normalize_group_name,
    },
    ids::{ActId, CategoryId, ClubId, RiderId},
};
use tracing::info;

use super::SqliteRepo;
use crate::{
    group_rules::{group_name_is_free, group_owner_access, load_group, type_conflict_message},
    registration::club_access,
};

#[async_trait::async_trait]
impl GroupsRepo for SqliteRepo {
    async fn create_group(
        &self,
        actor: &Actor,
        club: ClubId,
        act_type: ActType,
        name: &str,
    ) -> Result<ActId, RegistrationError> {
        let mut tx = self.begin_write().await?;

        club_access(&mut tx, actor, club).await?;
        ensure_group_type(act_type)?;
        let group_name = normalize_group_name(name).map_err(RegistrationError::Rejected)?;
        group_name_is_free(&mut tx, &group_name, None).await?;

        let act_id = ActId::new();
        info!(
            "Creating {} {group_name:?} ({act_id}) for club {club}",
            act_type.as_str(),
        );
        sqlx::query!(
            r#"
        INSERT INTO acts (id, name, act_type, group_name, owner_club_id)
        VALUES ($1, '', $2, $3, $4)
        "#,
            act_id,
            act_type,
            group_name,
            club,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(act_id)
    }

    async fn rename_group(
        &self,
        actor: &Actor,
        act: ActId,
        name: &str,
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;

        let group = load_group(&mut tx, act).await?;
        group_owner_access(&mut tx, actor, &group).await?;
        let group_name = normalize_group_name(name).map_err(RegistrationError::Rejected)?;
        group_name_is_free(&mut tx, &group_name, Some(group.id)).await?;

        info!("Renaming group {:?} to {group_name:?}", group.group_name);
        sqlx::query!(
            "UPDATE acts SET group_name = $1 WHERE id = $2",
            group_name,
            group.id,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn delete_group(&self, actor: &Actor, act: ActId) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;

        let group = load_group(&mut tx, act).await?;
        group_owner_access(&mut tx, actor, &group).await?;

        if !actor.manage_registrations {
            let other_club_riders = sqlx::query_scalar!(
                r#"
            SELECT COUNT(*) as "count!: i64"
            FROM act_participants
            JOIN rider ON rider.id = act_participants.rider_id
            WHERE act_participants.act_id = $1 AND rider.club_id IS NOT $2
            "#,
                group.id,
                group.owner_club_id,
            )
            .fetch_one(&mut *tx)
            .await?;
            if other_club_riders > 0 {
                return Err(RegistrationError::Rejected(format!(
                    "Die Gruppe „{}“ kann nicht gelöscht werden, solange noch FahrerInnen anderer Vereine in ihr angemeldet sind.",
                    group.group_name
                )));
            }
        }

        info!("Deleting group {:?} ({})", group.group_name, group.id);
        sqlx::query!("DELETE FROM act_participants WHERE act_id = $1", group.id)
            .execute(&mut *tx)
            .await?;
        sqlx::query!("DELETE FROM acts WHERE id = $1", group.id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn set_group_members(
        &self,
        actor: &Actor,
        club: ClubId,
        act: ActId,
        members: &[GroupMember],
    ) -> Result<Option<String>, RegistrationError> {
        let mut tx = self.begin_write().await?;

        club_access(&mut tx, actor, club).await?;
        let group = load_group(&mut tx, act).await?;

        let mut seen = HashSet::new();
        for member in members {
            if !seen.insert(member.rider_id) {
                return Err(RegistrationError::Rejected(
                    "Jede FahrerIn kann nur einmal in der Gruppe angemeldet werden.".to_string(),
                ));
            }
            let club_id = sqlx::query_scalar!(
                r#"SELECT club_id as "club_id!: ClubId" FROM rider WHERE id = $1"#,
                member.rider_id
            )
            .fetch_optional(&mut *tx)
            .await?;
            if club_id != Some(club) {
                return Err(RegistrationError::Rejected(
                    "Ein Verein kann nur seine eigenen FahrerInnen zu einer Gruppe hinzufügen."
                        .to_string(),
                ));
            }
        }

        info!(
            "Setting {} riders of club {club} in group {:?}",
            members.len(),
            group.group_name
        );
        sqlx::query!(
            r#"
        DELETE FROM act_participants
        WHERE act_id = $1 AND rider_id IN (SELECT id FROM rider WHERE club_id = $2)
        "#,
            group.id,
            club,
        )
        .execute(&mut *tx)
        .await?;

        for member in members {
            sqlx::query!(
                r#"
            INSERT INTO act_participants (act_id, rider_id, is_replacement)
            VALUES ($1, $2, $3)
            "#,
                group.id,
                member.rider_id,
                member.is_replacement,
            )
            .execute(&mut *tx)
            .await?;
        }

        let warning = type_conflict_message(&mut tx, group.id).await?;

        tx.commit().await?;
        Ok(warning)
    }

    async fn set_group_type(
        &self,
        actor: &Actor,
        act: ActId,
        act_type: ActType,
    ) -> Result<Option<String>, RegistrationError> {
        let mut tx = self.begin_write().await?;

        let group = load_group(&mut tx, act).await?;
        group_owner_access(&mut tx, actor, &group).await?;
        ensure_group_type(act_type)?;

        if group.act_type != act_type {
            info!(
                "Changing group {:?} from {} to {}",
                group.group_name,
                group.act_type.as_str(),
                act_type.as_str()
            );
            sqlx::query!(
                "UPDATE acts SET act_type = $1 WHERE id = $2",
                act_type,
                group.id,
            )
            .execute(&mut *tx)
            .await?;
        }
        let warning = type_conflict_message(&mut tx, group.id).await?;

        tx.commit().await?;
        Ok(warning)
    }

    async fn list_club_groups(
        &self,
        actor: &Actor,
        club: ClubId,
    ) -> Result<Vec<ClubGroup>, RegistrationError> {
        let pool = self.pool().await;
        let mut connection = pool.acquire().await?;
        club_access(&mut connection, actor, club).await?;

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
        .fetch_all(&mut *connection)
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
        .fetch_all(&mut *connection)
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
            let conflict = type_conflict_message(&mut connection, group.id).await?;
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

    async fn list_groups(&self) -> Result<Vec<GroupSummary>, DbError> {
        let pool = self.pool().await;
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
        .fetch_all(&pool)
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

    async fn group_conflict(&self, act: ActId) -> Result<Option<String>, DbError> {
        let pool = self.pool().await;
        let mut conn = pool.acquire().await?;
        Ok(type_conflict_message(&mut conn, act).await?)
    }
}
