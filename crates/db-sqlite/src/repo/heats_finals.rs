use std::collections::HashMap;

use db_core::{
    DbError, HeatsFinalsRepo,
    act_type::ActType,
    acts::ActParticipant,
    clubs::RegistrationError,
    finals::{
        Final, FinalChange, FinalSlot, NewFinal, SlotPlacement, finalist_label, group_slot_infos,
    },
    heats::{Discipline, Heat, HeatChange, HeatFinal, NewHeat},
    ids::{ActId, CategoryId, FinalId, FinalSlotId, HeatId},
    structure::{
        StructureAct, StructureCategory, StructureFinal, StructureHeat, StructureRows,
        StructureSlots,
    },
    structure_rules::{self, PlacedAct, PlacementSlot},
};
use tracing::info;

use super::SqliteRepo;
use crate::structure::{
    category_names, check_final_free, check_final_heats, check_heat_categories, check_heat_free,
    default_threshold_of, final_has_started, final_name_of, finals_of_heat, heat_discipline,
    heat_has_started, heat_name_of, jury_unit_use, replace_final_slots, set_heat_categories,
};

#[async_trait::async_trait]
impl HeatsFinalsRepo for SqliteRepo {
    async fn list_heats(&self) -> Result<Vec<Heat>, DbError> {
        let pool = self.pool().await;
        // One read transaction, so the heats and their categories match.
        let mut tx = pool.begin().await?;
        let mut categories: HashMap<HeatId, Vec<(CategoryId, String, Discipline)>> = HashMap::new();
        for row in sqlx::query!(
            r#"
            SELECT heat_id as "heat_id!: HeatId", id as "id!: CategoryId", name as "name!", act_type as "act_type: ActType", is_single_male
            FROM categories
            WHERE heat_id IS NOT NULL
            ORDER BY "order" NULLS FIRST, name
            "#
        )
        .fetch_all(&mut *tx)
        .await?
        {
            categories.entry(row.heat_id).or_default().push((
                row.id,
                row.name,
                Discipline::new(row.act_type, row.is_single_male),
            ));
        }
        let mut finals: HashMap<HeatId, Vec<HeatFinal>> = HashMap::new();
        for row in sqlx::query!(
            r#"
            SELECT DISTINCT final_slots.heat_id as "heat_id!: HeatId", finals.id as "final_id!: FinalId", finals.name as "final_name!", finals.code as "final_code!"
            FROM final_slots
            JOIN finals ON finals.id = final_slots.final_id
            ORDER BY finals.name
            "#
        )
        .fetch_all(&mut *tx)
        .await?
        {
            finals.entry(row.heat_id).or_default().push(HeatFinal {
                id: row.final_id,
                name: row.final_name,
                code: row.final_code,
            });
        }
        let rows = sqlx::query!(
            r#"
            SELECT
                id as "id!: HeatId",
                name as "name!",
                code as "code!",
                description,
                threshold,
                skip,
                (
                    SELECT COUNT(*) FROM view_act
                    WHERE withdrawn_at IS NULL AND category_id IN (SELECT id FROM categories WHERE heat_id = heats.id)
                ) as "act_count!: i64"
            FROM heats
            ORDER BY
                (SELECT MIN("order") FROM categories WHERE heat_id = heats.id) IS NULL,
                (SELECT MIN("order") FROM categories WHERE heat_id = heats.id),
                name
            "#
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let categories = categories.remove(&row.id).unwrap_or_default();
                let discipline = categories.first().map(|(_, _, d)| *d);
                Heat {
                    finals: finals.remove(&row.id).unwrap_or_default(),
                    id: row.id,
                    category_ids: categories.iter().map(|(id, _, _)| *id).collect(),
                    categories: categories.into_iter().map(|(_, name, _)| name).collect(),
                    act_type: discipline.map(|d| d.act_type),
                    is_single_male: discipline.map(|d| d.single_male),
                    name: row.name,
                    code: row.code,
                    description: row.description,
                    threshold: row.threshold,
                    skip: row.skip,
                    act_count: row.act_count,
                    skip_suggested: !discipline.is_some_and(|d| d.is_group())
                        && row.act_count < row.threshold,
                }
            })
            .collect())
    }

    async fn add_heat(&self, heat: &NewHeat) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let categories = category_names(&mut tx, &heat.category_ids).await?;
        check_heat_free(&mut tx, None, &heat.name, &heat.code).await?;
        let threshold = match heat.threshold {
            Some(threshold) => threshold,
            None => default_threshold_of(&mut tx, &heat.category_ids).await?,
        };
        info!("Adding heat {:?}", heat.name);
        let id = HeatId::new();
        sqlx::query!(
            "INSERT INTO heats (id, name, code, description, threshold) VALUES ($1, $2, $3, $4, $5)",
            id,
            heat.name,
            heat.code,
            heat.description,
            threshold
        )
        .execute(&mut *tx)
        .await?;
        // A refusal rolls the new heat back with the transaction.
        check_heat_categories(&mut tx, id, &categories).await?;
        set_heat_categories(&mut tx, id, &categories).await?;
        // Groups start in their final: their Vorlauf is skipped from the start
        // (#345); the organizer may restore it.
        if structure_rules::heat_starts_skipped(heat_discipline(&mut tx, id).await?) {
            sqlx::query!("UPDATE heats SET skip = TRUE WHERE id = $1", id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn edit_heat(&self, change: &HeatChange) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let categories = category_names(&mut tx, &change.category_ids).await?;
        let Some(name) = heat_name_of(&mut tx, change.id).await? else {
            return Err(RegistrationError::NotFound);
        };
        // Without a new code the heat keeps its own.
        let new_code = match &change.new_code {
            Some(code) => code.clone(),
            None => {
                sqlx::query_scalar!(
                    r#"SELECT code as "code!" FROM heats WHERE id = $1"#,
                    change.id
                )
                .fetch_one(&mut *tx)
                .await?
            }
        };
        check_heat_categories(&mut tx, change.id, &categories).await?;
        set_heat_categories(&mut tx, change.id, &categories).await?;
        check_heat_free(&mut tx, Some(change.id), &change.new_name, &new_code).await?;
        if change.new_name != name {
            info!("Renaming heat {name:?} to {:?}", change.new_name);
        }
        sqlx::query!(
            "UPDATE heats SET name = $1, code = $2, description = $3, threshold = coalesce($4, threshold) WHERE id = $5",
            change.new_name,
            new_code,
            change.description,
            change.threshold,
            change.id
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn delete_heat(&self, id: HeatId) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let finals = finals_of_heat(&mut tx, id).await?;
        let finals: Vec<String> = finals.into_iter().map(|(name, _)| name).collect();
        let jury_use = jury_unit_use(&mut tx, "heat", id.into()).await?;
        structure_rules::check_heat_deletable(&finals, jury_use)?;
        info!("Deleting heat {id}");
        // Its categories leave it (foreign key).
        let result = sqlx::query!("DELETE FROM heats WHERE id = $1", id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(RegistrationError::NotFound);
        }
        tx.commit().await?;
        Ok(())
    }

    async fn set_heat_skip(&self, id: HeatId, skip: bool) -> Result<bool, RegistrationError> {
        let mut tx = self.begin_write().await?;
        let heat = sqlx::query!(
            r#"SELECT name as "name!", skip FROM heats WHERE id = $1"#,
            id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(RegistrationError::NotFound)?;
        let started = heat_has_started(&mut tx, id).await?;
        if !structure_rules::check_set_heat_skip(&heat.name, heat.skip, skip, started)? {
            return Ok(false);
        }
        info!(
            "Setting the Vorlauf of heat {:?} skipped: {skip}",
            heat.name
        );
        sqlx::query!("UPDATE heats SET skip = $1 WHERE id = $2", skip, id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    async fn list_finals(&self, with_unpublished: bool) -> Result<Vec<Final>, DbError> {
        let pool = self.pool().await;
        // One read transaction, so the finals and their slots match.
        let mut tx = pool.begin().await?;
        let mut slots: HashMap<FinalId, Vec<FinalSlot>> = HashMap::new();
        // The heat of each slot, by id, name and code, for the slot groups.
        let mut slot_heats: HashMap<FinalId, Vec<(HeatId, String, String)>> = HashMap::new();
        for row in sqlx::query!(
            r#"
            SELECT
                final_slots.id as "id!: FinalSlotId",
                final_slots.final_id as "final_id!: FinalId",
                final_slots.heat_id as "heat_id!: HeatId",
                heats.name as "heat!",
                heats.code as "heat_code!",
                number,
                act_id as "act_id: ActId",
                final_slots.started_at IS NOT NULL as "started!: bool",
                finals.placement_published
            FROM final_slots
            JOIN finals ON finals.id = final_slots.final_id
            JOIN heats ON heats.id = final_slots.heat_id
            ORDER BY finals."order", finals.name, position
            "#
        )
        .fetch_all(&mut *tx)
        .await?
        {
            slot_heats.entry(row.final_id).or_default().push((
                row.heat_id,
                row.heat.clone(),
                row.heat_code.clone(),
            ));
            slots.entry(row.final_id).or_default().push(FinalSlot {
                id: row.id,
                label: finalist_label(row.number, &row.heat_code),
                heat_id: row.heat_id,
                group_label: row.heat_code,
                number: row.number,
                act_id: row
                    .act_id
                    .filter(|_| with_unpublished || row.placement_published),
                started: row.started,
            });
        }
        let mut categories: HashMap<FinalId, Vec<String>> = HashMap::new();
        for row in sqlx::query!(
            r#"
            SELECT DISTINCT final_slots.final_id as "final_id!: FinalId", categories.name as "category!", categories."order"
            FROM final_slots
            JOIN categories ON categories.heat_id = final_slots.heat_id
            ORDER BY final_slots.final_id, categories."order" NULLS FIRST, categories.name
            "#
        )
        .fetch_all(&mut *tx)
        .await?
        {
            categories
                .entry(row.final_id)
                .or_default()
                .push(row.category);
        }
        let rows = sqlx::query!(
            r#"
            SELECT
                id as "id!: FinalId",
                name as "name!",
                code as "code!",
                description,
                act_type as "act_type: ActType",
                is_single_male,
                "order",
                einfahrzeit_seconds,
                act_duration_seconds,
                judge_duration_seconds,
                placement_published,
                (SELECT id FROM timeplan WHERE timeplan.final_id = finals.id) as "timeplan_id: i64"
            FROM finals
            ORDER BY "order", name
            "#
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let slots = slots.remove(&row.id).unwrap_or_default();
                Final {
                    slot_groups: group_slot_infos(
                        slot_heats
                            .remove(&row.id)
                            .unwrap_or_default()
                            .iter()
                            .map(|(id, name, code)| (*id, name.as_str(), code.as_str())),
                    ),
                    started: slots.iter().any(|slot| slot.started),
                    slots,
                    categories: categories.remove(&row.id).unwrap_or_default(),
                    id: row.id,
                    placement_published: row.placement_published,
                    name: row.name,
                    code: row.code,
                    description: row.description,
                    act_type: row.act_type,
                    is_single_male: row.is_single_male,
                    order: row.order,
                    einfahrzeit_seconds: row.einfahrzeit_seconds,
                    act_duration_seconds: row.act_duration_seconds,
                    judge_duration_seconds: row.judge_duration_seconds,
                    timeplan_id: row.timeplan_id,
                }
            })
            .collect())
    }

    async fn add_final(&self, new: &NewFinal) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        check_final_free(&mut tx, None, &new.name, &new.code).await?;
        info!("Adding final {:?}", new.name);
        let fields = &new.fields;
        let id = FinalId::new();
        sqlx::query!(
            r#"
            INSERT INTO finals (
                id, name, code, description, act_type, is_single_male, "order",
                einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds
            )
            VALUES ($1, $2, $3, $4, $5, $6, (SELECT COALESCE(MAX("order"), 0) + 1 FROM finals), $7, $8, $9)
            "#,
            id,
            new.name,
            new.code,
            fields.description,
            fields.act_type,
            fields.is_single_male,
            fields.einfahrzeit_seconds,
            fields.act_duration_seconds,
            fields.judge_duration_seconds,
        )
        .execute(&mut *tx)
        .await?;
        check_final_heats(
            &mut tx,
            id,
            fields.act_type,
            fields.is_single_male,
            &fields.slot_groups,
        )
        .await?;
        replace_final_slots(&mut tx, id, &fields.slot_groups).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn edit_final(&self, change: &FinalChange) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let fields = &change.fields;
        let heats = sqlx::query_scalar!(
            r#"SELECT heat_id as "heat_id!: HeatId" FROM final_slots WHERE final_id = $1 ORDER BY position"#,
            change.id
        )
        .fetch_all(&mut *tx)
        .await?;
        let Some(name) = final_name_of(&mut tx, change.id).await? else {
            return Err(RegistrationError::NotFound);
        };
        // Without a new code the final keeps its own.
        let new_code = match &change.new_code {
            Some(code) => code.clone(),
            None => {
                sqlx::query_scalar!(
                    r#"SELECT code as "code!" FROM finals WHERE id = $1"#,
                    change.id
                )
                .fetch_one(&mut *tx)
                .await?
            }
        };
        check_final_free(&mut tx, Some(change.id), &change.new_name, &new_code).await?;
        if change.new_name != name {
            info!("Renaming final {name:?} to {:?}", change.new_name);
        }
        sqlx::query!(
            r#"
            UPDATE finals
            SET
                name = $1,
                code = $2,
                description = $3,
                act_type = $4,
                is_single_male = $5,
                einfahrzeit_seconds = $6,
                act_duration_seconds = $7,
                judge_duration_seconds = $8
            WHERE id = $9
            "#,
            change.new_name,
            new_code,
            fields.description,
            fields.act_type,
            fields.is_single_male,
            fields.einfahrzeit_seconds,
            fields.act_duration_seconds,
            fields.judge_duration_seconds,
            change.id,
        )
        .execute(&mut *tx)
        .await?;
        check_final_heats(
            &mut tx,
            change.id,
            fields.act_type,
            fields.is_single_male,
            &fields.slot_groups,
        )
        .await?;
        let started = final_has_started(&mut tx, change.id).await?;
        if structure_rules::check_slot_groups_change(heats, &fields.slot_groups, started)? {
            info!("Replacing the slots of final {:?}", change.new_name);
            replace_final_slots(&mut tx, change.id, &fields.slot_groups).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn delete_final(&self, id: FinalId) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let started = final_has_started(&mut tx, id).await?;
        let jury_use = jury_unit_use(&mut tx, "final", id.into()).await?;
        structure_rules::check_final_deletable(started, jury_use)?;
        info!("Deleting final {id}");
        sqlx::query!("DELETE FROM timeplan WHERE final_id = $1", id)
            .execute(&mut *tx)
            .await?;
        // Its slots go with it (foreign key).
        let result = sqlx::query!("DELETE FROM finals WHERE id = $1", id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(RegistrationError::NotFound);
        }
        tx.commit().await?;
        Ok(())
    }

    async fn set_final_placement(
        &self,
        id: FinalId,
        changes: &[SlotPlacement],
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let Some(final_name) = final_name_of(&mut tx, id).await? else {
            return Err(RegistrationError::NotFound);
        };
        let slots: Vec<PlacementSlot> = sqlx::query!(
            r#"
            SELECT
                final_slots.id as "id!: FinalSlotId",
                final_slots.heat_id as "heat_id!: HeatId",
                heats.name as "heat_name!",
                heats.code as "heat_code!",
                number,
                act_id as "act_id: ActId",
                final_slots.started_at IS NOT NULL as "started!: bool"
            FROM final_slots
            JOIN heats ON heats.id = final_slots.heat_id
            WHERE final_id = $1
            "#,
            id
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|row| PlacementSlot {
            id: row.id,
            heat_id: row.heat_id,
            heat_name: row.heat_name,
            heat_code: row.heat_code,
            number: row.number,
            act_id: row.act_id,
            started: row.started,
        })
        .collect();
        // The heat of each category.
        let heats: HashMap<String, HeatId> = sqlx::query!(
            r#"SELECT name as "name!", heat_id as "heat_id!: HeatId" FROM categories WHERE heat_id IS NOT NULL"#
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|row| (row.name, row.heat_id))
        .collect();
        // The acts to place: withdrawn acts don't start, they count as
        // without category.
        let mut acts: HashMap<ActId, PlacedAct> = HashMap::new();
        for act_id in changes.iter().filter_map(|change| change.act_id) {
            let category = sqlx::query_scalar!(
                r#"SELECT CAST(CASE WHEN withdrawn_at IS NULL THEN category END AS TEXT) as "category?: String"
                FROM view_act WHERE id = $1"#,
                act_id
            )
            .fetch_optional(&mut *tx)
            .await?;
            acts.insert(
                act_id,
                match category {
                    None => PlacedAct::Missing,
                    Some(None) => PlacedAct::Unavailable,
                    Some(Some(category)) => PlacedAct::InCategory(category),
                },
            );
        }
        let changed = structure_rules::plan_placement(&slots, changes, &acts, &heats)?;
        info!(
            "Placing acts in {} slots of final {:?}",
            changed.len(),
            final_name
        );
        // Empty the changed slots first, so acts can swap slots without
        // clashing in the unique index.
        for (slot_id, _) in &changed {
            sqlx::query!(
                "UPDATE final_slots SET act_id = NULL WHERE id = $1",
                slot_id
            )
            .execute(&mut *tx)
            .await?;
        }
        for (slot_id, act_id) in &changed {
            sqlx::query!(
                "UPDATE final_slots SET act_id = $1 WHERE id = $2",
                act_id,
                slot_id
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn set_final_published(
        &self,
        id: FinalId,
        published: bool,
    ) -> Result<(), RegistrationError> {
        let mut tx = self.begin_write().await?;
        let started = final_has_started(&mut tx, id).await?;
        structure_rules::check_final_published(published, started)?;
        info!("Setting the placement of final {id} published: {published}");
        let result = sqlx::query!(
            "UPDATE finals SET placement_published = $1 WHERE id = $2",
            published,
            id
        )
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(RegistrationError::NotFound);
        }
        tx.commit().await?;
        Ok(())
    }

    async fn structure_rows(&self) -> Result<StructureRows, DbError> {
        let pool = self.pool().await;
        let mut tx = pool.begin().await?;
        let categories = sqlx::query!(
            r#"
            SELECT id as "id!: CategoryId", name as "name!", description, act_type as "act_type: ActType", is_single_male, heat_id as "heat_id: HeatId"
            FROM categories
            ORDER BY "order" NULLS FIRST, name
            "#
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|row| StructureCategory {
            id: row.id,
            name: row.name,
            description: row.description,
            discipline: Discipline::new(row.act_type, row.is_single_male),
            heat_id: row.heat_id,
        })
        .collect();
        let heats = sqlx::query!(
            r#"SELECT id as "id!: HeatId", name as "name!", code as "code!", description, skip FROM heats ORDER BY name"#
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|row| StructureHeat {
            id: row.id,
            name: row.name,
            code: row.code,
            description: row.description,
            skip: row.skip,
        })
        .collect();
        let finals = sqlx::query!(
            r#"
            SELECT
                id as "id!: FinalId", name as "name!", code as "code!", description,
                act_type as "act_type: ActType", is_single_male
            FROM finals
            ORDER BY "order", name
            "#
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|row| StructureFinal {
            id: row.id,
            name: row.name,
            code: row.code,
            description: row.description,
            discipline: Discipline::new(row.act_type, row.is_single_male),
        })
        .collect();
        let slot_groups = sqlx::query!(
            r#"
            SELECT final_id as "final_id!: FinalId", heat_id as "heat_id!: HeatId", COUNT(*) as "slots!: i64"
            FROM final_slots
            GROUP BY final_id, heat_id
            ORDER BY final_id, MIN(position)
            "#
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|row| StructureSlots {
            final_id: row.final_id,
            heat_id: row.heat_id,
            slots: row.slots,
        })
        .collect();
        let acts = sqlx::query!(
            r#"
            SELECT
                view_act.id as "id!: ActId",
                view_act."order" as "order",
                view_act.name,
                view_act.group_name,
                view_act.act_type as "act_type!: ActType",
                view_act.category_id as "category_id: CategoryId",
                view_act.participants as "participants!: sqlx::types::Json<Vec<ActParticipant>>",
                view_act.withdrawn_at IS NOT NULL as "withdrawn!: bool",
                COALESCE((
                    SELECT MIN(s.gender = 'male')
                    FROM rider s JOIN act_participants p ON p.rider_id = s.id
                    WHERE p.act_id = view_act.id
                ), FALSE) as "male!: bool"
            FROM view_act
            "#
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|row| StructureAct {
            id: row.id,
            order: row.order,
            name: row.name,
            group_name: row.group_name,
            category_id: row.category_id,
            discipline: Discipline::new(row.act_type, row.male),
            participants: row.participants.0,
            withdrawn: row.withdrawn,
        })
        .collect();
        tx.commit().await?;
        Ok(StructureRows {
            categories,
            heats,
            finals,
            slot_groups,
            acts,
        })
    }
}
