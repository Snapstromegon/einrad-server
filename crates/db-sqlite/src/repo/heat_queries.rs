//! `HeatQueries` for SQLite.

use std::collections::HashMap;

use db_core::{
    act_type::ActType,
    actor::Actor,
    acts::ActParticipant,
    error::QueryError,
    heats::Discipline,
    ids::{ActId, CategoryId, FinalId, HeatId},
    queries::heat::{
        Heat, HeatFinal, HeatQueries, StructureAct, StructureCategory, StructureFinal,
        StructureHeat, StructureRows, StructureSlots,
    },
};
use sqlx::SqliteConnection;

use super::{SqliteRepo, read_tx};

#[async_trait::async_trait]
impl HeatQueries for SqliteRepo {
    async fn list_heats(&self) -> Result<Vec<Heat>, QueryError> {
        // One read transaction, so the heats and their categories match.
        read_tx!(self, tx, { Ok(list_heats(&mut tx).await?) })
    }

    async fn get_structure(&self, _actor: &Actor) -> Result<StructureRows, QueryError> {
        read_tx!(self, tx, { Ok(get_structure(&mut tx).await?) })
    }
}

async fn list_heats(conn: &mut SqliteConnection) -> sqlx::Result<Vec<Heat>> {
    // One read transaction, so the heats and their categories match.
    let mut categories: HashMap<HeatId, Vec<(CategoryId, String, Discipline)>> = HashMap::new();
    for row in sqlx::query!(
            r#"
            SELECT heat_id as "heat_id!: HeatId", id as "id!: CategoryId", name as "name!", act_type as "act_type: ActType", is_single_male
            FROM categories
            WHERE heat_id IS NOT NULL
            ORDER BY "order" NULLS FIRST, name
            "#
        )
        .fetch_all(&mut *conn)
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
        .fetch_all(&mut *conn)
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
        .fetch_all(&mut *conn)
        .await?;
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

async fn get_structure(conn: &mut SqliteConnection) -> sqlx::Result<StructureRows> {
    let categories = sqlx::query!(
            r#"
            SELECT id as "id!: CategoryId", name as "name!", description, act_type as "act_type: ActType", is_single_male, heat_id as "heat_id: HeatId"
            FROM categories
            ORDER BY "order" NULLS FIRST, name
            "#
        )
        .fetch_all(&mut *conn)
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
        .fetch_all(&mut *conn)
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
    .fetch_all(&mut *conn)
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
        .fetch_all(&mut *conn)
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
    .fetch_all(&mut *conn)
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
    Ok(StructureRows {
        categories,
        heats,
        finals,
        slot_groups,
        acts,
    })
}
