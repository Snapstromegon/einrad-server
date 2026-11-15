//! Groups (Kleingruppe / Großgruppe): the pure rules, and the [`GroupsRepo`]
//! trait.
//!
//! A group is an act with `group_name` set. Riders from several clubs can form
//! one group: the club that created it (`owner_club_id`) renames and deletes
//! it, every club adds and removes only its own riders.

use crate::{
    DbError,
    act_type::ActType,
    clubs::{Actor, RegistrationError},
    ids::{ActId, ClubId, RiderId},
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
/// use db_core::groups::normalize_group_name;
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
/// use db_core::groups::group_name_sort_key;
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

/// Checks that `act_type` is a group type. Fails with a German message
/// otherwise.
pub fn ensure_group_type(act_type: ActType) -> Result<(), RegistrationError> {
    if act_type.is_group() {
        Ok(())
    } else {
        Err(RegistrationError::Rejected(
            "Eine Gruppe ist eine Kleingruppe oder eine Großgruppe.".to_string(),
        ))
    }
}

/// Fails with a German message if another group already uses the name
/// (case-insensitive, see [`group_name_key`]). `existing` are the names of
/// the other groups.
pub fn check_group_name_free<'a>(
    group_name: &str,
    existing: impl IntoIterator<Item = &'a str>,
) -> Result<(), RegistrationError> {
    let key = group_name_key(group_name);
    if existing.into_iter().any(|name| group_name_key(name) == key) {
        return Err(RegistrationError::Rejected(format!(
            "Der Gruppenname „{group_name}“ ist bereits vergeben."
        )));
    }
    Ok(())
}

/// The owner club (or the organizer) deletes a group, but not while riders
/// of other clubs are in it. `foreign_riders` counts the participants whose
/// club isn't the owner club.
pub fn check_group_deletable(
    actor: &Actor,
    group_name: &str,
    foreign_riders: i64,
) -> Result<(), RegistrationError> {
    if !actor.manage_registrations() && foreign_riders > 0 {
        return Err(RegistrationError::Rejected(format!(
            "Die Gruppe „{group_name}“ kann nicht gelöscht werden, solange noch FahrerInnen anderer Vereine in ihr angemeldet sind."
        )));
    }
    Ok(())
}

/// A club sets only its own riders in a group, each at most once.
/// `riders` are the members in order with their rider's club (`None` for a
/// rider that doesn't exist).
pub fn check_group_members(
    club: ClubId,
    riders: &[(RiderId, Option<ClubId>)],
) -> Result<(), RegistrationError> {
    let mut seen = std::collections::HashSet::new();
    for &(rider, rider_club) in riders {
        if !seen.insert(rider) {
            return Err(RegistrationError::Rejected(
                "Jede FahrerIn kann nur einmal in der Gruppe angemeldet werden.".to_string(),
            ));
        }
        if rider_club != Some(club) {
            return Err(RegistrationError::Rejected(
                "Ein Verein kann nur seine eigenen FahrerInnen zu einer Gruppe hinzufügen."
                    .to_string(),
            ));
        }
    }
    Ok(())
}

/// A rider of a group who is also in another group of the same type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeConflict {
    pub firstname: String,
    pub lastname: String,
    pub club_name: String,
    /// The other group's name.
    pub other_group: String,
}

/// The warning for a group whose riders are also in other groups of its
/// type; `None` without conflicts. `conflicts` come sorted by rider name.
pub fn type_conflict_warning(
    group_name: &str,
    act_type: ActType,
    conflicts: &[TypeConflict],
) -> Option<String> {
    if conflicts.is_empty() {
        return None;
    }
    let mut message = format!("{} „{group_name}“:", act_type.german_name());
    for conflict in conflicts {
        message.push_str(&format!(
            " „{} {}“ ({}) ist bereits in der {} „{}“ angemeldet.",
            conflict.firstname,
            conflict.lastname,
            conflict.club_name,
            act_type.german_name(),
            conflict.other_group,
        ));
    }
    message.push_str(
        " Jede FahrerIn kann nur in einer Kleingruppe und einer Großgruppe starten – auch als ErsatzfahrerIn. Bis das geklärt ist, bekommen die betroffenen Gruppen keine Kategorie und starten nicht.",
    );
    Some(message)
}

#[async_trait::async_trait]
pub trait GroupsRepo {
    /// A rider can be in at most one small and one large group, replacements
    /// included. The rule isn't enforced when riders change (clubs add their
    /// riders to a shared group one after another); groups breaking it get no
    /// category and the clubs this warning.
    ///
    /// `None` without conflicts, otherwise a German message naming the
    /// riders, their clubs and the other groups.
    async fn group_conflict(&self, act: ActId) -> Result<Option<String>, DbError>;
}
