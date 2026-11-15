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
    ids::{ActId, CategoryId, ClubId, RiderId},
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

/// A group with its members, as a club sees it.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ClubGroup {
    pub id: ActId,
    pub group_name: String,
    pub act_type: ActType,
    /// `null` for incomplete groups (fewer than 3 regular riders) and groups
    /// with a `conflict`.
    pub category_id: Option<CategoryId>,
    pub category: Option<String>,
    /// Age in whole years of the oldest rider (replacements included) on the
    /// determining day.
    pub max_age: Option<f64>,
    /// Set if a rider of the group is also in another group of the same type:
    /// a German message naming them. The group has no category until that's
    /// sorted out.
    pub conflict: Option<String>,
    /// The act's title.
    pub name: String,
    pub song_file: Option<String>,
    pub owner_club_id: Option<ClubId>,
    pub owner_club_name: Option<String>,
    /// Whether the queried club created the group (and may rename or delete it).
    pub is_owner: bool,
    /// Regular riders first, then replacements, each sorted by name.
    pub members: Vec<ClubGroupMember>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ClubGroupMember {
    pub rider_id: RiderId,
    pub firstname: String,
    pub lastname: String,
    pub club_id: ClubId,
    pub club_name: String,
    pub is_replacement: bool,
    /// Only for the queried club's own riders.
    #[serde(with = "time::serde::iso8601::option")]
    pub birthdate: Option<time::OffsetDateTime>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct GroupSummary {
    pub id: ActId,
    pub group_name: String,
    pub act_type: ActType,
    /// `null` for incomplete groups (fewer than 3 regular riders).
    pub category_id: Option<CategoryId>,
    pub category: Option<String>,
    pub owner_club_id: Option<ClubId>,
    pub owner_club_name: Option<String>,
    pub regular_count: i64,
    pub replacement_count: i64,
    /// The clubs with riders in the group, sorted by name.
    pub club_names: Vec<String>,
}

/// A rider a club sets in a group.
#[derive(Debug, Clone, Copy)]
pub struct GroupMember {
    pub rider_id: RiderId,
    /// Replacement riders (Ersatzfahrer) don't count towards the group size.
    pub is_replacement: bool,
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
    /// Creates a group act for the club, which becomes its owner. Group
    /// names are unique across the competition (case-insensitive).
    async fn create_group(
        &self,
        actor: &Actor,
        club: ClubId,
        act_type: ActType,
        name: &str,
    ) -> Result<ActId, RegistrationError>;

    /// Only the owner club (or the organizer) renames a group.
    async fn rename_group(
        &self,
        actor: &Actor,
        act: ActId,
        name: &str,
    ) -> Result<(), RegistrationError>;

    /// Deletes a group; its riders stay registered. The owner club can't
    /// delete it while riders of other clubs are in it.
    async fn delete_group(&self, actor: &Actor, act: ActId) -> Result<(), RegistrationError>;

    /// Makes `members` the club's riders in the group; riders of other clubs
    /// are kept. Returns the type conflict warning, if any (see
    /// [`GroupsRepo::group_conflict`]).
    async fn set_group_members(
        &self,
        actor: &Actor,
        club: ClubId,
        act: ActId,
        members: &[GroupMember],
    ) -> Result<Option<String>, RegistrationError>;

    /// Changes a group's type. Returns the type conflict warning, if any.
    async fn set_group_type(
        &self,
        actor: &Actor,
        act: ActId,
        act_type: ActType,
    ) -> Result<Option<String>, RegistrationError>;

    /// The groups the club created or has riders in.
    async fn list_club_groups(
        &self,
        actor: &Actor,
        club: ClubId,
    ) -> Result<Vec<ClubGroup>, RegistrationError>;

    /// All groups, without rider data.
    async fn list_groups(&self) -> Result<Vec<GroupSummary>, DbError>;

    /// A rider can be in at most one small and one large group, replacements
    /// included. The rule isn't enforced when riders change (clubs add their
    /// riders to a shared group one after another); groups breaking it get no
    /// category and the clubs this warning.
    ///
    /// `None` without conflicts, otherwise a German message naming the
    /// riders, their clubs and the other groups.
    async fn group_conflict(&self, act: ActId) -> Result<Option<String>, DbError>;
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

    fn actor(manage_registrations: bool) -> Actor {
        Actor::new(
            crate::ids::UserId::new(),
            "Max Müller",
            if manage_registrations {
                vec![crate::roles::Role::Admin]
            } else {
                vec![]
            },
            None,
        )
    }

    #[test]
    fn foreign_riders_block_deleting_a_group_unless_organizer() {
        assert!(check_group_deletable(&actor(false), "Wirbel", 0).is_ok());
        let blocked = check_group_deletable(&actor(false), "Wirbel", 1);
        assert!(matches!(blocked, Err(RegistrationError::Rejected(m)) if m.contains("„Wirbel“")));
        assert!(check_group_deletable(&actor(true), "Wirbel", 5).is_ok());
    }

    #[test]
    fn group_names_must_be_free_beyond_ascii_case() {
        assert!(check_group_name_free("Überflieger", ["Zebra", "Anker"]).is_ok());
        let err = check_group_name_free("Überflieger", ["Zebra", "überflieger"]).unwrap_err();
        assert!(matches!(
            err,
            RegistrationError::Rejected(m) if m == "Der Gruppenname „Überflieger“ ist bereits vergeben."
        ));
    }

    #[test]
    fn group_members_are_own_riders_and_unique() {
        let club = ClubId::new();
        let (a, b) = (RiderId::new(), RiderId::new());
        assert!(check_group_members(club, &[]).is_ok());
        assert!(check_group_members(club, &[(a, Some(club)), (b, Some(club))]).is_ok());
        let duplicate = check_group_members(club, &[(a, Some(club)), (a, Some(club))]);
        assert!(
            matches!(duplicate, Err(RegistrationError::Rejected(m)) if m.contains("nur einmal"))
        );
        for other in [Some(ClubId::new()), None] {
            let foreign = check_group_members(club, &[(a, Some(club)), (b, other)]);
            assert!(
                matches!(foreign, Err(RegistrationError::Rejected(m)) if m.contains("eigenen FahrerInnen"))
            );
        }
        // The duplicate comes first, like the loop it replaces.
        let both = check_group_members(club, &[(a, Some(club)), (a, None)]);
        assert!(matches!(both, Err(RegistrationError::Rejected(m)) if m.contains("nur einmal")));
    }

    #[test]
    fn type_conflicts_are_named_or_absent() {
        assert_eq!(
            type_conflict_warning("Wirbel", ActType::GroupSmall, &[]),
            None
        );
        let warning = type_conflict_warning(
            "Wirbel",
            ActType::GroupSmall,
            &[TypeConflict {
                firstname: "Anna".into(),
                lastname: "Muster".into(),
                club_name: "RSV Adorf".into(),
                other_group: "Sturm".into(),
            }],
        )
        .unwrap();
        assert!(warning.starts_with("Kleingruppe „Wirbel“: „Anna Muster“ (RSV Adorf) ist bereits in der Kleingruppe „Sturm“ angemeldet."));
        assert!(warning.ends_with("starten nicht."));
    }
}
