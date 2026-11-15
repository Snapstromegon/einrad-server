//! `AdminCommands` for SQLite. One write transaction per command.

use db_core::{
    actor::Actor,
    commands::admin::{AdminCommands, SetActWithdrawn},
    error::CommandError,
};
use sqlx::SqliteConnection;

use super::{SqliteRepo, write_tx};

#[async_trait::async_trait]
impl AdminCommands for SqliteRepo {
    async fn set_act_withdrawn(
        &self,
        _actor: &Actor,
        cmd: SetActWithdrawn,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, { set_act_withdrawn(&mut tx, cmd).await? });
        Ok(())
    }
}

async fn set_act_withdrawn(
    conn: &mut SqliteConnection,
    cmd: SetActWithdrawn,
) -> Result<(), CommandError> {
    let updated = sqlx::query!(
        r#"
        UPDATE acts
        SET withdrawn_at = CASE WHEN $1 THEN COALESCE(withdrawn_at, CURRENT_TIMESTAMP) END
        WHERE id = $2
        "#,
        cmd.withdrawn,
        cmd.act_id,
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(CommandError::NotFound);
    }
    Ok(())
}
