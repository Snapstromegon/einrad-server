//! The competition as the export reads it: the rows of the competition and
//! its structure, in one read transaction.

use db_core::{
    DbError, ExportRepo,
    act_type::ActType,
    ids::{FinalId, HeatId},
    jury::UnitSettings,
    seed::{CompetitionDump, DumpCategory, DumpEntry, DumpFinal, DumpHeat, DumpImage},
    timeplan::EntryKind,
};
use time::OffsetDateTime;

use super::SqliteRepo;
use crate::{competition, images};

#[async_trait::async_trait]
impl ExportRepo for SqliteRepo {
    async fn dump_competition(&self) -> Result<Option<CompetitionDump>, DbError> {
        let pool = self.pool().await;
        let mut conn = pool.begin().await?;
        let conn = &mut *conn;
        let Some(competition) = competition::current(&mut *conn).await? else {
            return Ok(None);
        };
        let mut dump_images = Vec::new();
        for image in images::list(&mut *conn).await? {
            let stored = if image.source_url.is_none() && image.has_data {
                images::load(&mut *conn, image.id).await?
            } else {
                None
            };
            dump_images.push(DumpImage { image, stored });
        }

        let categories = sqlx::query!(
            r#"
            SELECT
                name as "name!", description, act_type as "act_type: ActType",
                is_single_male, min_age, under_age, einfahrzeit_seconds,
                act_duration_seconds, judge_duration_seconds
            FROM categories ORDER BY "order" NULLS FIRST, name
            "#
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|row| DumpCategory {
            name: row.name,
            description: row.description,
            act_type: row.act_type,
            single_male: row.is_single_male,
            min_age: row.min_age,
            under_age: row.under_age,
            einfahrzeit_seconds: row.einfahrzeit_seconds,
            act_duration_seconds: row.act_duration_seconds,
            judge_duration_seconds: row.judge_duration_seconds,
        })
        .collect();

        // In the order of their first category, empty ones last, as the
        // admin UI lists them.
        let mut heats = Vec::new();
        for row in sqlx::query!(
            r#"
            SELECT
                id as "id!: HeatId", name as "name!", code as "code!", description, threshold, skip,
                jury_registrable as "jury_registrable: bool", jury_deployments,
                jury_needed_technical, jury_needed_performance, jury_needed_dismounts
            FROM heats
            ORDER BY
                (SELECT MIN("order") FROM categories WHERE heat_id = heats.id) IS NULL,
                (SELECT MIN("order") FROM categories WHERE heat_id = heats.id),
                name
            "#
        )
        .fetch_all(&mut *conn)
        .await?
        {
            let categories = sqlx::query_scalar!(
                r#"SELECT name as "name!" FROM categories WHERE heat_id = $1 ORDER BY "order" NULLS FIRST, name"#,
                row.id
            )
            .fetch_all(&mut *conn)
            .await?;
            heats.push(DumpHeat {
                name: row.name,
                code: row.code,
                description: row.description,
                threshold: row.threshold,
                skip: row.skip,
                jury: UnitSettings {
                    registrable: row.jury_registrable,
                    deployments: row.jury_deployments,
                    needed_technical: row.jury_needed_technical,
                    needed_performance: row.jury_needed_performance,
                    needed_dismounts: row.jury_needed_dismounts,
                },
                categories,
            });
        }

        let rows = sqlx::query!(
            r#"
            SELECT
                id as "id!: FinalId", name as "name!", code as "code!", description, act_type as "act_type: ActType",
                is_single_male, einfahrzeit_seconds, act_duration_seconds,
                judge_duration_seconds,
                jury_registrable as "jury_registrable: bool", jury_deployments,
                jury_needed_technical, jury_needed_performance, jury_needed_dismounts
            FROM finals ORDER BY "order", name
            "#
        )
        .fetch_all(&mut *conn)
        .await?;
        let mut finals = Vec::with_capacity(rows.len());
        for row in rows {
            let slot_heats = sqlx::query_scalar!(
                r#"
                SELECT heats.code as "heat!" FROM final_slots
                JOIN heats ON heats.id = final_slots.heat_id
                WHERE final_slots.final_id = $1 ORDER BY position
                "#,
                row.id
            )
            .fetch_all(&mut *conn)
            .await?;
            finals.push(DumpFinal {
                name: row.name,
                code: row.code,
                description: row.description,
                act_type: row.act_type,
                single_male: row.is_single_male,
                einfahrzeit_seconds: row.einfahrzeit_seconds,
                act_duration_seconds: row.act_duration_seconds,
                judge_duration_seconds: row.judge_duration_seconds,
                jury: UnitSettings {
                    registrable: row.jury_registrable,
                    deployments: row.jury_deployments,
                    needed_technical: row.jury_needed_technical,
                    needed_performance: row.jury_needed_performance,
                    needed_dismounts: row.jury_needed_dismounts,
                },
                slot_heats,
            });
        }

        let timeplan = sqlx::query!(
            r#"
            SELECT
                kind as "kind: EntryKind",
                (SELECT name FROM categories WHERE categories.id = timeplan.category_id) as category,
                (SELECT code FROM finals WHERE finals.id = timeplan.final_id) as final_name,
                label, duration_seconds,
                einfahrzeit_seconds, show_photos, internal,
                earliest_start_time as "earliest_start_time: OffsetDateTime"
            FROM timeplan ORDER BY position, id
            "#
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|row| DumpEntry {
            kind: row.kind,
            category: row.category,
            final_code: row.final_name,
            label: row.label,
            duration_seconds: row.duration_seconds,
            einfahrzeit_seconds: row.einfahrzeit_seconds,
            show_photos: row.show_photos,
            internal: row.internal,
            earliest_start: row.earliest_start_time,
        })
        .collect();

        Ok(Some(CompetitionDump {
            competition,
            images: dump_images,
            categories,
            heats,
            finals,
            timeplan,
        }))
    }
}
