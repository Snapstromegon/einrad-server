//! `CompetitionCommands` for SQLite.

use db_core::{
    actor::Actor,
    branding::Branding,
    commands::competition::{CompetitionCommands, EditCompetition},
    error::CommandError,
};
use sqlx::SqliteConnection;

use super::{SqliteRepo, write_tx};
use crate::competition;

#[async_trait::async_trait]
impl CompetitionCommands for SqliteRepo {
    async fn edit_competition(
        &self,
        _actor: &Actor,
        cmd: EditCompetition,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, { edit_competition(&mut tx, cmd).await? });
        Ok(())
    }
}

/// Writes the edited settings of the current competition (the id of the
/// request is ignored) and both colors; the current competition and the write
/// share one transaction.
async fn edit_competition(
    conn: &mut SqliteConnection,
    cmd: EditCompetition,
) -> Result<(), CommandError> {
    let Some(id) = competition::current_id(&mut *conn).await? else {
        return Err(CommandError::NotFound);
    };
    let edited = db_core::competition::Competition {
        id,
        ..cmd.competition
    };
    competition::upsert(&mut *conn, &edited).await?;
    // The form sets both colors; `None` is the default (ADR 0009).
    competition::write_colors(
        &mut *conn,
        &edited.id,
        &Branding::set_or_reset(edited.primary_color.clone()),
        &Branding::set_or_reset(edited.secondary_color.clone()),
    )
    .await?;
    Ok(())
}
