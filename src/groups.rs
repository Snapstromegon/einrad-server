//! Helpers for group acts (Kleingruppe / Großgruppe).
//!
//! A group is an act with `group_name` set. Riders from several clubs can form
//! one group: the club that created it (`owner_club_id`) renames and deletes
//! it, every club adds and removes only its own riders.
//!
//! The helpers take a `&mut SqliteConnection`, so commands can run them inside
//! their transaction and roll back when a check fails.

use axum::http::StatusCode;
use sqlx::SqliteConnection;
use uuid::Uuid;

use crate::{
    act_type::ActType,
    http_server::{HttpError, extractor::auth::Auth},
    roles::Permission,
};

/// Group names longer than this are rejected.
pub const MAX_GROUP_NAME_LENGTH: usize = 60;

/// Whether the character may not appear in a group name: control characters
/// and the bidi controls that reorder the displayed text.
fn is_forbidden_in_group_name(c: char) -> bool {
    c.is_control()
        || ('\u{202A}'..='\u{202E}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
}

/// Trims the group name and collapses inner whitespace. The returned error is
/// a user-facing (German) message.
///
/// Names starting with `=`, `+` or `@` are rejected, so spreadsheets don't
/// read them as formulas in the startlist export, and so are control and bidi
/// characters.
///
/// ```
/// use nrw_freestyle_cup_registration::groups::normalize_group_name;
///
/// assert_eq!(normalize_group_name("  Die   Wirbelwinde ").unwrap(), "Die Wirbelwinde");
/// assert!(normalize_group_name("   ").is_err());
/// assert!(normalize_group_name("=HYPERLINK(\"x\")").is_err());
/// ```
pub fn normalize_group_name(name: &str) -> Result<String, String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err("Bitte gib einen Gruppennamen an.".to_string());
    }
    if name.chars().count() > MAX_GROUP_NAME_LENGTH {
        return Err(format!(
            "Der Gruppenname darf höchstens {MAX_GROUP_NAME_LENGTH} Zeichen lang sein."
        ));
    }
    if name.starts_with(['=', '+', '@']) || name.chars().any(is_forbidden_in_group_name) {
        return Err(
            "Der Gruppenname enthält unerlaubte Zeichen. Er darf nicht mit =, + oder @ beginnen."
                .to_string(),
        );
    }
    Ok(name)
}

/// Compares group names like the uniqueness rule does: ignoring case,
/// including non-ASCII letters (SQLite's `NOCASE` only folds A–Z).
pub fn group_name_key(name: &str) -> String {
    name.to_lowercase()
}

/// Sort key for lists of groups: ignores case and sorts letters with umlauts
/// or accents like their base letter, so „Überflieger“ comes before „Zebra“.
/// Ties are broken by the lowercased name.
///
/// ```
/// use nrw_freestyle_cup_registration::groups::group_name_sort_key;
///
/// let mut names = ["Zebra", "überflieger", "Uhu", "Ärger", "anker"];
/// names.sort_by_key(|name| group_name_sort_key(name));
/// assert_eq!(names, ["anker", "Ärger", "überflieger", "Uhu", "Zebra"]);
/// ```
pub fn group_name_sort_key(name: &str) -> (String, String) {
    let lower = name.to_lowercase();
    let mut folded = String::with_capacity(lower.len());
    for c in lower.chars() {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => folded.push('a'),
            'æ' => folded.push_str("ae"),
            'ç' => folded.push('c'),
            'è' | 'é' | 'ê' | 'ë' => folded.push('e'),
            'ì' | 'í' | 'î' | 'ï' => folded.push('i'),
            'ñ' => folded.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => folded.push('o'),
            'œ' => folded.push_str("oe"),
            'ù' | 'ú' | 'û' | 'ü' => folded.push('u'),
            'ý' | 'ÿ' => folded.push('y'),
            'ß' => folded.push_str("ss"),
            c => folded.push(c),
        }
    }
    (folded, lower)
}

/// Checks that the user may manage the club: admins, the club's owner and
/// users assigned to the club.
///
/// Fails with 404 for an unknown club and 403 without access.
pub async fn ensure_club_access(
    conn: &mut SqliteConnection,
    auth: &Auth,
    club_id: Uuid,
) -> Result<(), HttpError> {
    let owner_id = sqlx::query_scalar!(
        r#"SELECT owner_id as "owner_id!: Uuid" FROM clubs WHERE id = ?"#,
        club_id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;

    if auth.can(Permission::ManageRegistrations)
        || owner_id == auth.user_id
        || auth.club_id == Some(club_id)
    {
        Ok(())
    } else {
        Err(HttpError::StatusCode(StatusCode::FORBIDDEN))
    }
}

/// Loads the club of a starter and checks that the user may manage it (see
/// [`ensure_club_access`]). Returns the club's id.
///
/// Fails with 404 for an unknown starter and 403 without access.
pub async fn ensure_starter_access(
    conn: &mut SqliteConnection,
    auth: &Auth,
    starter_id: Uuid,
) -> Result<Uuid, HttpError> {
    let club_id = sqlx::query_scalar!(
        r#"SELECT club_id as "club_id!: Uuid" FROM starter WHERE id = ?"#,
        starter_id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;
    ensure_club_access(conn, auth, club_id).await?;
    Ok(club_id)
}

/// Checks that the user may change the act (title, description, song):
/// admins and users with access to the club of one of its riders or, for
/// groups, to the club that created it.
///
/// Fails with 404 for an unknown act and 403 without access.
pub async fn ensure_act_access(
    conn: &mut SqliteConnection,
    auth: &Auth,
    act_id: Uuid,
) -> Result<(), HttpError> {
    let allowed = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM clubs
            WHERE (clubs.owner_id = ?2 OR clubs.id = ?3)
                AND (
                    clubs.id IN (
                        SELECT starter.club_id FROM act_participants
                        JOIN starter ON starter.id = act_participants.starter_id
                        WHERE act_participants.act_id = acts.id
                    )
                    OR clubs.id = acts.owner_club_id
                )
        ) as "allowed!: bool"
        FROM acts
        WHERE acts.id = ?1
        "#,
        act_id,
        auth.user_id,
        auth.club_id,
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;

    if allowed || auth.can(Permission::ManageRegistrations) {
        Ok(())
    } else {
        Err(HttpError::StatusCode(StatusCode::FORBIDDEN))
    }
}

/// A group act as needed by the group commands.
#[derive(Debug)]
pub struct Group {
    pub id: Uuid,
    pub group_name: String,
    pub act_type: ActType,
    pub owner_club_id: Option<Uuid>,
}

/// Loads a group act. Fails with 404 if the act doesn't exist or isn't a group.
pub async fn get_group(conn: &mut SqliteConnection, act_id: Uuid) -> Result<Group, HttpError> {
    sqlx::query_as!(
        Group,
        r#"
        SELECT
            id as "id!: Uuid",
            group_name as "group_name!",
            act_type as "act_type!: ActType",
            owner_club_id as "owner_club_id: Uuid"
        FROM acts
        WHERE id = ? AND group_name IS NOT NULL
        "#,
        act_id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))
}

/// Checks that the user may rename or delete the group: admins and users with
/// access to the group's owner club. Fails with 403 otherwise.
pub async fn ensure_group_owner(
    conn: &mut SqliteConnection,
    auth: &Auth,
    group: &Group,
) -> Result<(), HttpError> {
    if auth.can(Permission::ManageRegistrations) {
        return Ok(());
    }
    match group.owner_club_id {
        Some(owner_club_id) => ensure_club_access(conn, auth, owner_club_id).await,
        None => Err(HttpError::StatusCode(StatusCode::FORBIDDEN)),
    }
}

/// Fails with a German message if another group already uses the name
/// (case-insensitive, see [`group_name_key`]). `except_act_id` excludes the
/// group being renamed.
pub async fn ensure_group_name_is_free(
    conn: &mut SqliteConnection,
    group_name: &str,
    except_act_id: Option<Uuid>,
) -> Result<(), HttpError> {
    // The NOCASE index only folds ASCII letters, so „Ü“ and „ü“ are compared
    // here. There are only a few dozen groups.
    let existing = sqlx::query_scalar!(
        r#"
        SELECT group_name as "group_name!" FROM acts
        WHERE group_name IS NOT NULL AND id IS NOT ?
        "#,
        except_act_id
    )
    .fetch_all(&mut *conn)
    .await?;
    let key = group_name_key(group_name);
    if existing.iter().any(|name| group_name_key(name) == key) {
        return Err(HttpError::ErrorMessages(format!(
            "Der Gruppenname „{group_name}“ ist bereits vergeben."
        )));
    }
    Ok(())
}

/// Checks that `act_type` is a group type, for commands that set a group's
/// type. Fails with a German message otherwise.
pub fn ensure_group_type(act_type: ActType) -> Result<(), HttpError> {
    if act_type.is_group() {
        Ok(())
    } else {
        Err(HttpError::ErrorMessages(
            "Eine Gruppe ist eine Kleingruppe oder eine Großgruppe.".to_string(),
        ))
    }
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
pub async fn type_conflict_message(
    conn: &mut SqliteConnection,
    act_id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    let conflicts = sqlx::query!(
        r#"
        SELECT
            starter.firstname,
            starter.lastname,
            clubs.name as club_name,
            this.act_type as "act_type!: ActType",
            other.group_name
        FROM act_participants own
        JOIN acts this ON this.id = own.act_id
        JOIN act_participants theirs
            ON theirs.starter_id = own.starter_id AND theirs.act_id != own.act_id
        JOIN acts other ON other.id = theirs.act_id AND other.act_type = this.act_type
        JOIN starter ON starter.id = own.starter_id
        JOIN clubs ON clubs.id = starter.club_id
        WHERE own.act_id = ? AND this.act_type IN ('group_small', 'group_large')
        ORDER BY starter.lastname, starter.firstname, other.group_name
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
        WHERE id = ?
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_names_are_trimmed_and_whitespace_is_collapsed() {
        assert_eq!(
            normalize_group_name("\t Rollende \n Rebellen  ").unwrap(),
            "Rollende Rebellen"
        );
    }

    #[test]
    fn empty_group_names_are_rejected() {
        for name in ["", "   ", "\n\t"] {
            let err = normalize_group_name(name).unwrap_err();
            assert!(err.contains("Gruppennamen"), "{name:?}: {err}");
        }
    }

    #[test]
    fn group_name_length_is_counted_in_characters() {
        let longest = "Ä".repeat(MAX_GROUP_NAME_LENGTH);
        assert_eq!(normalize_group_name(&longest).unwrap(), longest);
        let err = normalize_group_name(&format!("{longest}x")).unwrap_err();
        assert!(err.contains("höchstens"), "{err}");
    }

    #[test]
    fn formula_starts_and_control_characters_are_rejected() {
        for name in [
            "=HYPERLINK(\"http://x\";\"Klick\")",
            " +49",
            "@Wirbel",
            "Wirbel\u{0}wind",
            "Wirbel\u{202E}dniw",
            "Wirbel\u{2066}wind",
        ] {
            let err = normalize_group_name(name).unwrap_err();
            assert!(err.contains("unerlaubte Zeichen"), "{name:?}: {err}");
        }
        // Inside the name, and a leading minus, are fine.
        for name in ["Wind;Wetter", "A=B", "-Minus-", "Rock & Roll @ Home"] {
            assert_eq!(normalize_group_name(name).unwrap(), name);
        }
    }

    #[test]
    fn group_name_keys_ignore_case_beyond_ascii() {
        assert_eq!(group_name_key("Überflieger"), group_name_key("überflieger"));
        assert_eq!(group_name_key("ÉLAN"), group_name_key("élan"));
        assert_ne!(group_name_key("Uberflieger"), group_name_key("Überflieger"));
    }
}
