//! The group rules on a connection. The repo methods run them inside their
//! write transaction and roll back when a check fails.

use db_core::{
    act_type::ActType,
    clubs::{Actor, RegistrationError},
    groups::group_name_key,
    ids::{ActId, ClubId},
};
use sqlx::SqliteConnection;

use crate::registration::club_access;

/// A group act as needed by the group commands.
#[derive(Debug)]
pub(crate) struct Group {
    pub(crate) id: ActId,
    pub(crate) group_name: String,
    pub(crate) act_type: ActType,
    pub(crate) owner_club_id: Option<ClubId>,
}

/// Loads a group act. Fails with 404 if the act doesn't exist or isn't a group.
pub(crate) async fn load_group(
    conn: &mut SqliteConnection,
    act_id: ActId,
) -> Result<Group, RegistrationError> {
    sqlx::query_as!(
        Group,
        r#"
        SELECT
            id as "id!: ActId",
            group_name as "group_name!",
            act_type as "act_type!: ActType",
            owner_club_id as "owner_club_id: ClubId"
        FROM acts
        WHERE id = $1 AND group_name IS NOT NULL
        "#,
        act_id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(RegistrationError::NotFound)
}

/// Checks that the user may rename or delete the group: admins and users with
/// access to the group's owner club. Fails with 403 otherwise.
pub(crate) async fn group_owner_access(
    conn: &mut SqliteConnection,
    actor: &Actor,
    group: &Group,
) -> Result<(), RegistrationError> {
    if actor.manage_registrations {
        return Ok(());
    }
    match group.owner_club_id {
        Some(owner_club_id) => club_access(conn, actor, owner_club_id).await,
        None => Err(RegistrationError::Forbidden),
    }
}

/// Fails with a German message if another group already uses the name
/// (case-insensitive, see [`group_name_key`]). `except_act_id` excludes the
/// group being renamed.
pub(crate) async fn group_name_is_free(
    conn: &mut SqliteConnection,
    group_name: &str,
    except_act_id: Option<ActId>,
) -> Result<(), RegistrationError> {
    // The NOCASE index only folds ASCII letters, so „Ü“ and „ü“ are compared
    // here. There are only a few dozen groups.
    let existing = sqlx::query_scalar!(
        r#"
        SELECT group_name as "group_name!" FROM acts
        WHERE group_name IS NOT NULL AND id IS NOT $1
        "#,
        except_act_id
    )
    .fetch_all(&mut *conn)
    .await?;
    let key = group_name_key(group_name);
    if existing.iter().any(|name| group_name_key(name) == key) {
        return Err(RegistrationError::Rejected(format!(
            "Der Gruppenname „{group_name}“ ist bereits vergeben."
        )));
    }
    Ok(())
}

/// A rider can be in at most one small and one large group, replacements
/// included.
///
/// The rule isn't enforced when riders change: clubs add their riders to a
/// shared group one after another. Instead, groups breaking the rule get no
/// category (see `view_act`) and the clubs get this warning.
///
/// Checks every participant of the act (from any club) against all other acts
/// of the same type. Returns `None` without conflicts, otherwise a user-facing
/// (German) message naming the riders, their clubs and the other groups.
pub(crate) async fn type_conflict_message(
    conn: &mut SqliteConnection,
    act_id: ActId,
) -> Result<Option<String>, sqlx::Error> {
    let conflicts = sqlx::query!(
        r#"
        SELECT
            rider.firstname,
            rider.lastname,
            clubs.name as club_name,
            this.act_type as "act_type!: ActType",
            other.group_name
        FROM act_participants own
        JOIN acts this ON this.id = own.act_id
        JOIN act_participants theirs
            ON theirs.rider_id = own.rider_id AND theirs.act_id != own.act_id
        JOIN acts other ON other.id = theirs.act_id AND other.act_type = this.act_type
        JOIN rider ON rider.id = own.rider_id
        JOIN clubs ON clubs.id = rider.club_id
        WHERE own.act_id = $1 AND this.act_type IN ('group_small', 'group_large')
        ORDER BY rider.lastname, rider.firstname, other.group_name
        "#,
        act_id
    )
    .fetch_all(&mut *conn)
    .await?;

    if conflicts.is_empty() {
        return Ok(None);
    }

    let group = sqlx::query!(
        r#"
        SELECT group_name as "group_name!", act_type as "act_type!: ActType"
        FROM acts
        WHERE id = $1
        "#,
        act_id
    )
    .fetch_one(&mut *conn)
    .await?;
    let mut message = format!("{} „{}“:", group.act_type.german_name(), group.group_name);
    for conflict in &conflicts {
        message.push_str(&format!(
            " „{} {}“ ({}) ist bereits in der {} „{}“ angemeldet.",
            conflict.firstname,
            conflict.lastname,
            conflict.club_name,
            conflict.act_type.german_name(),
            conflict.group_name.as_deref().unwrap_or_default(),
        ));
    }
    message.push_str(
        " Jede FahrerIn kann nur in einer Kleingruppe und einer Großgruppe starten – auch als ErsatzfahrerIn. Bis das geklärt ist, bekommen die betroffenen Gruppen keine Kategorie und starten nicht.",
    );
    Ok(Some(message))
}
