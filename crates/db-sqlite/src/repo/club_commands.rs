//! `ClubCommands` for SQLite: riders, pairs and singles of a club. One write
//! transaction per command; access is decided inside it.

use db_core::{
    actor::Actor,
    commands::club::{
        AddRider, ClubCommands, DeleteRider, DissolvePair, EditRider, LeavePair, LinkPair,
        ReplaceActRider, RequestPair, SetSingle,
    },
    error::CommandError,
    ids::{ActId, RiderId},
    registration_rules::{self as rules, PartnerClubs},
};
use sqlx::SqliteConnection;

use super::{SqliteRepo, write_tx};
use crate::registration::{
    self, act_access, club_access, ensure_rider_name_is_free, load_act_participants, map_duplicate,
    rider_access,
};

#[async_trait::async_trait]
impl ClubCommands for SqliteRepo {
    async fn add_rider(&self, actor: &Actor, cmd: AddRider) -> Result<RiderId, CommandError> {
        Ok(write_tx!(self, tx, {
            add_rider(&mut tx, actor, cmd).await?
        }))
    }

    async fn edit_rider(&self, actor: &Actor, cmd: EditRider) -> Result<(), CommandError> {
        write_tx!(self, tx, { edit_rider(&mut tx, actor, cmd).await? });
        Ok(())
    }

    async fn delete_rider(&self, actor: &Actor, cmd: DeleteRider) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            rider_access(&mut tx, actor, cmd.rider_id).await?;
            registration::delete_rider(&mut tx, cmd.rider_id).await?;
        });
        Ok(())
    }

    async fn set_single(&self, actor: &Actor, cmd: SetSingle) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            rider_access(&mut tx, actor, cmd.rider_id).await?;
            registration::set_single(&mut tx, cmd.rider_id, cmd.single).await?;
        });
        Ok(())
    }

    async fn request_pair(
        &self,
        actor: &Actor,
        cmd: RequestPair,
    ) -> Result<Option<RiderId>, CommandError> {
        Ok(write_tx!(self, tx, {
            rider_access(&mut tx, actor, cmd.rider_id).await?;
            registration::request_pair(&mut tx, cmd.rider_id, cmd.partner_name.as_deref()).await?
        }))
    }

    async fn link_pair(&self, actor: &Actor, cmd: LinkPair) -> Result<ActId, CommandError> {
        Ok(write_tx!(self, tx, {
            rider_access(&mut tx, actor, cmd.rider_id).await?;
            registration::link_pair(
                &mut tx,
                cmd.rider_id,
                cmd.partner_id,
                PartnerClubs::of(actor),
            )
            .await?
        }))
    }

    async fn leave_pair(&self, actor: &Actor, cmd: LeavePair) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            rider_access(&mut tx, actor, cmd.rider_id).await?;
            registration::leave_pair(&mut tx, cmd.rider_id).await?;
        });
        Ok(())
    }

    async fn dissolve_pair(&self, actor: &Actor, cmd: DissolvePair) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            act_access(&mut tx, actor, cmd.act_id).await?;
            registration::dissolve_pair(&mut tx, cmd.act_id).await?;
        });
        Ok(())
    }

    async fn replace_act_rider(
        &self,
        actor: &Actor,
        cmd: ReplaceActRider,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, { replace_act_rider(&mut tx, actor, cmd).await? });
        Ok(())
    }
}

async fn add_rider(
    conn: &mut SqliteConnection,
    actor: &Actor,
    cmd: AddRider,
) -> Result<RiderId, CommandError> {
    club_access(conn, actor, cmd.club_id).await?;
    ensure_rider_name_is_free(conn, cmd.club_id, &cmd.firstname, &cmd.lastname, None).await?;

    let rider_id = RiderId::new();
    sqlx::query!(
        r#"
        INSERT INTO rider (id, club_id, firstname, lastname, birthdate, gender)
        VALUES ($1, $2, $3, $4, $5, $6);
        "#,
        rider_id,
        cmd.club_id,
        cmd.firstname,
        cmd.lastname,
        cmd.birthdate,
        cmd.gender,
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| map_duplicate(e, &cmd.firstname, &cmd.lastname))?;
    Ok(rider_id)
}

async fn edit_rider(
    conn: &mut SqliteConnection,
    actor: &Actor,
    cmd: EditRider,
) -> Result<(), CommandError> {
    let club_id = rider_access(conn, actor, cmd.rider_id).await?;
    ensure_rider_name_is_free(
        conn,
        club_id,
        &cmd.firstname,
        &cmd.lastname,
        Some(cmd.rider_id),
    )
    .await?;

    sqlx::query!(
        r#"
        UPDATE rider SET firstname = $1, lastname = $2, birthdate = $3, gender = COALESCE($4, gender)
        WHERE id = $5
        "#,
        cmd.firstname,
        cmd.lastname,
        cmd.birthdate,
        cmd.gender,
        cmd.rider_id,
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| map_duplicate(e, &cmd.firstname, &cmd.lastname))?;
    Ok(())
}

async fn replace_act_rider(
    conn: &mut SqliteConnection,
    actor: &Actor,
    cmd: ReplaceActRider,
) -> Result<(), CommandError> {
    let ReplaceActRider {
        act_id: act,
        rider_id: old,
        new_rider_id: new,
    } = cmd;
    let (withdrawn, participants) = load_act_participants(conn, act)
        .await?
        .ok_or(CommandError::NotFound)?;
    rules::check_act_open(withdrawn)?;

    let club_id = rider_access(conn, actor, old).await?;
    let new_club_id = rider_access(conn, actor, new).await?;
    rules::check_replacement_club(actor, club_id, new_club_id)?;
    rules::check_act_participants(&participants, old, new)?;

    registration::replace_rider(conn, act, old, new).await?;
    Ok(())
}
