//! `CheckInCommands` and `CheckInQueries` for SQLite: the check-in desk.

use std::collections::HashSet;

use db_core::{
    actor::Actor,
    commands::check_in::{CheckInCommands, SetCheckedIn},
    error::{CommandError, QueryError},
    ids::RiderId,
    queries::check_in::{CheckIn, CheckInQueries},
};
use sqlx::SqliteConnection;
use time::OffsetDateTime;

use super::{SqliteRepo, read_tx, write_tx};

#[async_trait::async_trait]
impl CheckInCommands for SqliteRepo {
    async fn set_checked_in(&self, _actor: &Actor, cmd: SetCheckedIn) -> Result<(), CommandError> {
        write_tx!(self, tx, { set_checked_in(&mut tx, cmd).await? });
        Ok(())
    }
}

#[async_trait::async_trait]
impl CheckInQueries for SqliteRepo {
    async fn list_check_ins(&self, _actor: &Actor) -> Result<Vec<CheckIn>, QueryError> {
        read_tx!(self, tx, { Ok(list_check_ins(&mut tx).await?) })
    }
}

async fn set_checked_in(
    conn: &mut SqliteConnection,
    cmd: SetCheckedIn,
) -> Result<(), CommandError> {
    let riders: HashSet<RiderId> = cmd.rider_ids.iter().copied().collect();
    for rider_id in riders {
        let result = if let Some(checked_in_at) = cmd.checked_in_at {
            sqlx::query!(
                "UPDATE rider SET checked_in_at = COALESCE(checked_in_at, $1) WHERE id = $2",
                checked_in_at,
                rider_id,
            )
            .execute(&mut *conn)
            .await?
        } else {
            sqlx::query!(
                "UPDATE rider SET checked_in_at = NULL WHERE id = $1",
                rider_id
            )
            .execute(&mut *conn)
            .await?
        };
        if result.rows_affected() == 0 {
            return Err(CommandError::NotFound);
        }
    }
    Ok(())
}

async fn list_check_ins(conn: &mut SqliteConnection) -> sqlx::Result<Vec<CheckIn>> {
    sqlx::query_as!(
        CheckIn,
        r#"
        SELECT
            id as "rider_id!: RiderId",
            checked_in_at as "checked_in_at: OffsetDateTime"
        FROM rider
        "#
    )
    .fetch_all(&mut *conn)
    .await
}
