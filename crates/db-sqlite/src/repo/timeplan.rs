use db_core::{DbError, TimeplanRepo, ids::ActId};
use sqlx::SqliteConnection;

use super::{SqliteRepo, write_tx};

/// What the player still takes from the timeplan area (#238); it moves to
/// `Support` with the player.
#[async_trait::async_trait]
impl TimeplanRepo for SqliteRepo {
    async fn mark_music_started(&self, act_id: ActId, seconds_ago: f64) -> Result<bool, DbError> {
        Ok(write_tx!(self, tx, {
            mark_music_started_in(&mut tx, act_id, seconds_ago).await?
        }))
    }
}

/// Records that the music of a running act (or its finalist slot) started
/// `seconds_ago`, unless it did before. Whether it was recorded.
pub(super) async fn mark_music_started_in(
    conn: &mut SqliteConnection,
    act_id: ActId,
    seconds_ago: f64,
) -> sqlx::Result<bool> {
    let act = sqlx::query!(
            r#"
            UPDATE acts SET music_started_at = datetime(julianday('now') - $1 / 86400.0)
            WHERE id = $2 AND started_at IS NOT NULL AND ended_at IS NULL AND music_started_at IS NULL
            "#,
            seconds_ago,
            act_id
        )
        .execute(&mut *conn)
        .await?;
    let finalist = sqlx::query!(
            r#"
            UPDATE final_slots SET music_started_at = datetime(julianday('now') - $1 / 86400.0)
            WHERE act_id = $2 AND started_at IS NOT NULL AND ended_at IS NULL AND music_started_at IS NULL
                AND final_id IN (SELECT id FROM finals WHERE placement_published)
            "#,
            seconds_ago,
            act_id
        )
        .execute(&mut *conn)
        .await?;
    Ok(act.rows_affected() + finalist.rows_affected() > 0)
}
