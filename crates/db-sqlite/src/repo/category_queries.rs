//! `CategoryQueries` for SQLite.

use db_core::{
    act_type::ActType,
    actor::Actor,
    error::QueryError,
    ids::{CategoryId, HeatId},
    queries::category::{Category, CategoryQueries},
};
use sqlx::SqliteConnection;

use super::{SqliteRepo, read_tx};

#[async_trait::async_trait]
impl CategoryQueries for SqliteRepo {
    async fn list_categories(&self, _actor: Option<&Actor>) -> Result<Vec<Category>, QueryError> {
        read_tx!(self, tx, { Ok(list_categories(&mut tx).await?) })
    }
}

async fn list_categories(conn: &mut SqliteConnection) -> sqlx::Result<Vec<Category>> {
    sqlx::query_as!(
            Category,
            r#"
            SELECT id as "id!: CategoryId", name as "name!", description, min_age, under_age, act_type as "act_type: ActType", is_single_male, "order", einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds, heat_id as "heat_id: HeatId", (SELECT name FROM heats WHERE heats.id = categories.heat_id) as heat
            FROM categories ORDER BY "order" ASC NULLS FIRST
            "#
        )
        .fetch_all(&mut *conn)
        .await
}
