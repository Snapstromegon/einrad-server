//! `GroupCommands` for SQLite: groups of several riders. One write transaction
//! per command; access is decided inside it.

use db_core::{
    actor::Actor,
    commands::group::{
        CreateClubGroup, DeleteClubGroup, GroupCommands, GroupMember, RenameClubGroup,
        SetClubGroupMembers, SetClubGroupType,
    },
    error::CommandError,
    groups::{check_group_deletable, check_group_members, ensure_group_type, normalize_group_name},
    ids::{ActId, ClubId},
};
use sqlx::SqliteConnection;
use tracing::info;

use super::{SqliteRepo, write_tx};
use crate::{
    group_rules::{group_name_is_free, group_owner_access, load_group, type_conflict_message},
    registration::club_access,
};

#[async_trait::async_trait]
impl GroupCommands for SqliteRepo {
    async fn create_club_group(
        &self,
        actor: &Actor,
        cmd: CreateClubGroup,
    ) -> Result<ActId, CommandError> {
        Ok(write_tx!(self, tx, {
            create_club_group(&mut tx, actor, cmd).await?
        }))
    }

    async fn rename_club_group(
        &self,
        actor: &Actor,
        cmd: RenameClubGroup,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, { rename_club_group(&mut tx, actor, cmd).await? });
        Ok(())
    }

    async fn delete_club_group(
        &self,
        actor: &Actor,
        cmd: DeleteClubGroup,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, { delete_club_group(&mut tx, actor, cmd).await? });
        Ok(())
    }

    async fn set_club_group_members(
        &self,
        actor: &Actor,
        cmd: SetClubGroupMembers,
    ) -> Result<Option<String>, CommandError> {
        Ok(write_tx!(self, tx, {
            set_club_group_members(&mut tx, actor, cmd).await?
        }))
    }

    async fn set_club_group_type(
        &self,
        actor: &Actor,
        cmd: SetClubGroupType,
    ) -> Result<Option<String>, CommandError> {
        Ok(write_tx!(self, tx, {
            set_club_group_type(&mut tx, actor, cmd).await?
        }))
    }
}

async fn create_club_group(
    conn: &mut SqliteConnection,
    actor: &Actor,
    cmd: CreateClubGroup,
) -> Result<ActId, CommandError> {
    let CreateClubGroup {
        club_id: club,
        act_type,
        group_name: name,
    } = cmd;
    club_access(conn, actor, club).await?;
    ensure_group_type(act_type)?;
    let group_name = normalize_group_name(&name).map_err(CommandError::Rejected)?;
    group_name_is_free(conn, &group_name, None).await?;

    let act_id = ActId::new();
    info!(
        "Creating {} {group_name:?} ({act_id}) for club {club}",
        act_type.as_str(),
    );
    sqlx::query!(
        r#"
        INSERT INTO acts (id, name, act_type, group_name, owner_club_id)
        VALUES ($1, '', $2, $3, $4)
        "#,
        act_id,
        act_type,
        group_name,
        club,
    )
    .execute(&mut *conn)
    .await?;
    Ok(act_id)
}

async fn rename_club_group(
    conn: &mut SqliteConnection,
    actor: &Actor,
    cmd: RenameClubGroup,
) -> Result<(), CommandError> {
    let group = load_group(conn, cmd.act_id).await?;
    group_owner_access(conn, actor, &group).await?;
    let group_name = normalize_group_name(&cmd.group_name).map_err(CommandError::Rejected)?;
    group_name_is_free(conn, &group_name, Some(group.id)).await?;

    info!("Renaming group {:?} to {group_name:?}", group.group_name);
    sqlx::query!(
        "UPDATE acts SET group_name = $1 WHERE id = $2",
        group_name,
        group.id,
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn delete_club_group(
    conn: &mut SqliteConnection,
    actor: &Actor,
    cmd: DeleteClubGroup,
) -> Result<(), CommandError> {
    let group = load_group(conn, cmd.act_id).await?;
    group_owner_access(conn, actor, &group).await?;

    let foreign_riders = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) as "count!: i64"
        FROM act_participants
        JOIN rider ON rider.id = act_participants.rider_id
        WHERE act_participants.act_id = $1 AND rider.club_id IS NOT $2
        "#,
        group.id,
        group.owner_club_id,
    )
    .fetch_one(&mut *conn)
    .await?;
    check_group_deletable(actor, &group.group_name, foreign_riders)?;

    info!("Deleting group {:?} ({})", group.group_name, group.id);
    sqlx::query!("DELETE FROM act_participants WHERE act_id = $1", group.id)
        .execute(&mut *conn)
        .await?;
    sqlx::query!("DELETE FROM acts WHERE id = $1", group.id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

async fn set_club_group_members(
    conn: &mut SqliteConnection,
    actor: &Actor,
    cmd: SetClubGroupMembers,
) -> Result<Option<String>, CommandError> {
    let SetClubGroupMembers {
        club_id: club,
        act_id: act,
        members,
    } = cmd;
    club_access(conn, actor, club).await?;
    let group = load_group(conn, act).await?;

    let mut riders = Vec::with_capacity(members.len());
    for member in &members {
        let club_id = sqlx::query_scalar!(
            r#"SELECT club_id as "club_id!: ClubId" FROM rider WHERE id = $1"#,
            member.rider_id
        )
        .fetch_optional(&mut *conn)
        .await?;
        riders.push((member.rider_id, club_id));
    }
    check_group_members(club, &riders)?;

    info!(
        "Setting {} riders of club {club} in group {:?}",
        members.len(),
        group.group_name
    );
    sqlx::query!(
        r#"
        DELETE FROM act_participants
        WHERE act_id = $1 AND rider_id IN (SELECT id FROM rider WHERE club_id = $2)
        "#,
        group.id,
        club,
    )
    .execute(&mut *conn)
    .await?;

    for GroupMember {
        rider_id,
        is_replacement,
    } in members
    {
        sqlx::query!(
            r#"
            INSERT INTO act_participants (act_id, rider_id, is_replacement)
            VALUES ($1, $2, $3)
            "#,
            group.id,
            rider_id,
            is_replacement,
        )
        .execute(&mut *conn)
        .await?;
    }

    Ok(type_conflict_message(conn, group.id).await?)
}

async fn set_club_group_type(
    conn: &mut SqliteConnection,
    actor: &Actor,
    cmd: SetClubGroupType,
) -> Result<Option<String>, CommandError> {
    let SetClubGroupType { act_id, act_type } = cmd;
    let group = load_group(conn, act_id).await?;
    group_owner_access(conn, actor, &group).await?;
    ensure_group_type(act_type)?;

    if group.act_type != act_type {
        info!(
            "Changing group {:?} from {} to {}",
            group.group_name,
            group.act_type.as_str(),
            act_type.as_str()
        );
        sqlx::query!(
            "UPDATE acts SET act_type = $1 WHERE id = $2",
            act_type,
            group.id,
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(type_conflict_message(conn, group.id).await?)
}
