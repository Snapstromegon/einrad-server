//! `FinalQueries` for SQLite.

use std::collections::HashMap;

use db_core::{
    act_type::ActType,
    actor::Actor,
    error::QueryError,
    finals::finalist_label,
    ids::{ActId, FinalId, FinalSlotId, HeatId},
    queries::finals::{
        Final, FinalQueries, FinalSlot, group_slot_infos, sees_unpublished_placement,
    },
};
use sqlx::SqliteConnection;

use super::{SqliteRepo, read_tx};

#[async_trait::async_trait]
impl FinalQueries for SqliteRepo {
    async fn list_finals(&self, actor: Option<&Actor>) -> Result<Vec<Final>, QueryError> {
        let with_unpublished = sees_unpublished_placement(actor);
        // One read transaction, so the finals and their slots match.
        read_tx!(self, tx, {
            Ok(list_finals(&mut tx, with_unpublished).await?)
        })
    }
}

async fn list_finals(
    conn: &mut SqliteConnection,
    with_unpublished: bool,
) -> sqlx::Result<Vec<Final>> {
    // One read transaction, so the finals and their slots match.
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
    .fetch_all(&mut *conn)
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
        .fetch_all(&mut *conn)
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
    .fetch_all(&mut *conn)
    .await?;
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
