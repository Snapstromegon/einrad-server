//! `TimeplanQueries` for SQLite.

use db_core::{
    actor::Actor,
    error::QueryError,
    ids::{CategoryId, FinalId},
    queries::timeplan::{
        ListTimeplan, PredictTimeplan, StartlistCsv, TimeplanListEntry, TimeplanQueries,
    },
    timeplan::{Audience, EntryKind, INTERNAL_PUBLIC_LABEL, Timeplan},
};
use sqlx::SqliteConnection;

use super::{SqliteRepo, acts::list_startlist_in, read_tx};
use crate::timeplan::load_for;

#[async_trait::async_trait]
impl TimeplanQueries for SqliteRepo {
    async fn list_timeplan(
        &self,
        _actor: Option<&Actor>,
        query: ListTimeplan,
    ) -> Result<Vec<TimeplanListEntry>, QueryError> {
        read_tx!(self, tx, {
            Ok(list_timeplan(&mut tx, query.audience).await?)
        })
    }

    async fn predict_timeplan(
        &self,
        _actor: Option<&Actor>,
        query: PredictTimeplan,
    ) -> Result<Timeplan, QueryError> {
        // One read transaction, so the blocks and acts are read consistently.
        read_tx!(self, tx, { Ok(load_for(&mut tx, query.audience).await?) })
    }

    async fn get_startlist_csv(&self, _actor: Option<&Actor>) -> Result<StartlistCsv, QueryError> {
        read_tx!(self, tx, {
            // A printed startlist is public: internal entries are a "Pause".
            let timeplan = load_for(&mut tx, Audience::Public).await?;
            let acts = list_startlist_in(&mut tx).await?;
            Ok(StartlistCsv { timeplan, acts })
        })
    }
}

async fn list_timeplan(
    conn: &mut SqliteConnection,
    audience: Audience,
) -> sqlx::Result<Vec<TimeplanListEntry>> {
    // The public gets internal entries as a "Pause", like in the prediction.
    let public = audience == Audience::Public;
    sqlx::query_as!(
            TimeplanListEntry,
            r#"
            SELECT
                id,
                kind as "kind: EntryKind",
                earliest_start_time,
                duration_seconds,
                CASE WHEN internal AND ?1 THEN ?2 ELSE label END as "label: String",
                category_id as "category_id: CategoryId",
                (SELECT name FROM categories WHERE categories.id = timeplan.category_id) as category,
                final_id as "final_id: FinalId",
                (SELECT name FROM finals WHERE finals.id = timeplan.final_id) as final_name,
                einfahrzeit_seconds,
                show_photos,
                internal AND NOT ?1 as "internal!: bool",
                EXISTS (
                    SELECT 1 FROM timeplan_slots WHERE timeplan_slots.timeplan_id = timeplan.id
                ) as "manually_ordered!: bool",
                started_at,
                ended_at
            FROM timeplan
            ORDER BY position, id
            "#,
            public,
            INTERNAL_PUBLIC_LABEL,
        )
        .fetch_all(&mut *conn)
        .await
}
