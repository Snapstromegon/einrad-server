//! Heats (Vorlaufklassen, #338): a named group of categories of one discipline
//! whose acts are ranked together to fill a final's finalist slots.
//!
//! HEAT-1: all categories of a heat share one act type and, for singles,
//! gender; a category belongs to at most one heat (`categories.heat_id`). Group
//! categories belong to heats like any other (#345, ADR 0015); a heat of
//! groups has its Vorlauf skipped from the start. A heat may be empty until
//! the admin assigns its categories. Its discipline follows from its
//! categories; an empty heat has none and fits any final.
//!
//! A final's slot groups refer to heats (`final_slots.heat_id`, see
//! `crate::finals`); the final's categories are those of its heats. Clubs
//! never see or pick a heat: it follows from the category.

use crate::act_type::ActType;
use crate::ids::{CategoryId, FinalId, HeatId};

/// The act type and, for singles, gender that all categories of a heat
/// share. Only singles are male or female: other act types are never male.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Discipline {
    pub act_type: ActType,
    pub single_male: bool,
}

impl Discipline {
    pub fn new(act_type: ActType, single_male: bool) -> Self {
        Self {
            act_type,
            single_male: single_male && act_type == ActType::Single,
        }
    }

    pub fn is_group(self) -> bool {
        self.act_type.is_group()
    }
}

/// A category as far as heats care.
#[derive(Debug, Clone, PartialEq)]
pub struct CategoryInfo {
    pub name: String,
    pub discipline: Discipline,
    /// The name of the heat it belongs to, if any.
    pub heat: Option<String>,
}

/// Why categories can't form a heat (HEAT-1), naming the offender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// No category of that name.
    Unknown(String),
    /// Of another act type or gender than the first category.
    OtherDiscipline { category: String, first: String },
    /// Already in another heat.
    InOtherHeat { category: String, heat: String },
}

impl Problem {
    /// The problem in German, for the admin UI.
    pub fn german(&self) -> String {
        match self {
            Problem::Unknown(category) => format!("Die Kategorie {category} gibt es nicht."),
            Problem::OtherDiscipline { category, first } => format!(
                "{category} hat einen anderen Typ oder ein anderes Geschlecht als {first}; alle Kategorien einer Vorlaufklasse teilen beides."
            ),
            Problem::InOtherHeat { category, heat } => {
                format!("{category} gehört schon zur Vorlaufklasse „{heat}“.")
            }
        }
    }

    /// The problem in English, for the competition file.
    pub fn english(&self) -> String {
        match self {
            Problem::Unknown(category) => {
                format!("category {category} isn't a category of the file")
            }
            Problem::OtherDiscipline { category, first } => format!(
                "{category} has another act type or gender than {first}; a heat's categories share both"
            ),
            Problem::InOtherHeat { category, heat } => format!(
                "{category} is already in heat {heat}; a category belongs to at most one heat"
            ),
        }
    }
}

/// Checks that `categories` can form the heat `heat` (HEAT-1) among `known`
/// categories, and gives their discipline; `None` for an empty heat.
/// Categories already in `heat` are fine.
///
/// ```
/// use db_core::act_type::ActType;
/// use db_core::heats::{CategoryInfo, Discipline, Problem, check};
///
/// let male = Discipline::new(ActType::Single, true);
/// let known = [
///     CategoryInfo { name: "EMU17".into(), discipline: male, heat: None },
///     CategoryInfo { name: "PU17".into(), discipline: Discipline::new(ActType::Pair, false), heat: None },
/// ];
/// let names = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
/// assert_eq!(check("E EM 1", &names(&["EMU17"]), &known), Ok(Some(male)));
/// assert_eq!(check("E EM 1", &[], &known), Ok(None));
/// assert_eq!(
///     check("E EM 1", &names(&["EMU17", "PU17"]), &known),
///     Err(Problem::OtherDiscipline { category: "PU17".into(), first: "EMU17".into() })
/// );
/// ```
pub fn check(
    heat: &str,
    categories: &[String],
    known: &[CategoryInfo],
) -> Result<Option<Discipline>, Problem> {
    let mut first: Option<&CategoryInfo> = None;
    for name in categories {
        let Some(category) = known.iter().find(|c| &c.name == name) else {
            return Err(Problem::Unknown(name.clone()));
        };
        if let Some(other) = category.heat.as_deref().filter(|other| *other != heat) {
            return Err(Problem::InOtherHeat {
                category: name.clone(),
                heat: other.to_string(),
            });
        }
        match first {
            Some(first) if first.discipline != category.discipline => {
                return Err(Problem::OtherDiscipline {
                    category: name.clone(),
                    first: first.name.clone(),
                });
            }
            Some(_) => {}
            None => first = Some(category),
        }
    }
    Ok(first.map(|category| category.discipline))
}

/// The fewest acts a Junior Expert heat (U15 and younger) needs for its
/// Vorlauf (Konzept §1.5).
pub const JUNIOR_EXPERT_THRESHOLD: i64 = 6;
/// The fewest acts any other heat needs for its Vorlauf (Konzept §1.5).
pub const EXPERT_THRESHOLD: i64 = 5;
/// The oldest `under_age` of a Junior Expert category: U15.
const JUNIOR_EXPERT_UNDER_AGE: f64 = 15.0;

/// The threshold a heat gets when it is created without one, from the
/// `under_age` of its categories: [`JUNIOR_EXPERT_THRESHOLD`] if all are
/// U15 or younger, otherwise (also without categories)
/// [`EXPERT_THRESHOLD`]. It is stored and the organizer can change it.
pub fn default_threshold(under_ages: impl IntoIterator<Item = Option<f64>>) -> i64 {
    let mut under_ages = under_ages.into_iter().peekable();
    let junior = under_ages.peek().is_some()
        && under_ages.all(|age| age.is_some_and(|age| age <= JUNIOR_EXPERT_UNDER_AGE));
    if junior {
        JUNIOR_EXPERT_THRESHOLD
    } else {
        EXPERT_THRESHOLD
    }
}

/// Checks a threshold sent by the admin UI. The message is German.
pub fn check_threshold(threshold: i64) -> Result<i64, String> {
    if threshold < 0 {
        Err("Die Mindestzahl an Küren kann nicht negativ sein.".to_string())
    } else {
        Ok(threshold)
    }
}

/// Trims a heat's name; fails with a German message if it's empty.
pub fn normalize_name(name: &str) -> Result<String, String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        Err("Eine Vorlaufklasse braucht einen Namen.".to_string())
    } else {
        Ok(name)
    }
}

/// The short code of a heat or final, trimmed: the one given, or else the
/// name. The code is what files refer to and what labels use where space is
/// tight (ADR 0017).
pub fn normalize_code(code: Option<&str>, name: &str) -> String {
    let collapse = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(collapse(code.unwrap_or_default()))
        .filter(|code| !code.is_empty())
        .unwrap_or_else(|| collapse(name))
}

/// Trims the category names of a heat and drops empty and repeated ones,
/// keeping their order.
pub fn normalize_categories(categories: &[String]) -> Vec<String> {
    let mut result: Vec<String> = vec![];
    for category in categories {
        let category = category.trim();
        if !category.is_empty() && !result.iter().any(|c| c == category) {
            result.push(category.to_string());
        }
    }
    result
}

/// A final a heat fills slots of, as the heat list shows it.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct HeatFinal {
    pub id: FinalId,
    /// The readable name.
    pub name: String,
    /// The short code.
    pub code: String,
}

/// A heat as the API lists it.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Heat {
    pub id: HeatId,
    /// The readable name, unique, e.g. "Einzel männlich – Expert 1 (U17–U21)".
    pub name: String,
    /// The short code, unique, e.g. "E EM 1".
    pub code: String,
    pub description: Option<String>,
    /// Its categories, in category order. Empty until the admin assigns
    /// them.
    pub categories: Vec<String>,
    /// The same categories by id, in the same order.
    pub category_ids: Vec<CategoryId>,
    /// The act type its categories share; `None` without categories.
    pub act_type: Option<ActType>,
    /// Whether its categories are singles male; `None` without categories.
    pub is_single_male: Option<bool>,
    /// The finals whose slots it fills; at most one.
    pub finals: Vec<HeatFinal>,
    /// The fewest acts it needs for its Vorlauf.
    pub threshold: i64,
    /// Whether its Vorlauf is skipped: its acts start directly in its final.
    pub skip: bool,
    /// Its acts at the moment, without withdrawn ones.
    pub act_count: i64,
    /// Whether the tool suggests skipping its Vorlauf: it has fewer acts than
    /// its threshold; never for groups, which start in their final. The
    /// organizer decides.
    pub skip_suggested: bool,
}

/// A heat to add; name, code and description are normalized.
#[derive(Debug, Clone)]
pub struct NewHeat {
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub category_ids: Vec<CategoryId>,
    /// Without: the default for its categories, see [`default_threshold`].
    pub threshold: Option<i64>,
}

/// What an edit of a heat sets; name and description are normalized.
#[derive(Debug, Clone)]
pub struct HeatChange {
    pub id: HeatId,
    pub new_name: String,
    /// The new normalized code; without, it stays.
    pub new_code: Option<String>,
    pub description: Option<String>,
    /// The categories of the heat; the ones left out leave it.
    pub category_ids: Vec<CategoryId>,
    /// Without, it stays.
    pub threshold: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str, act_type: ActType, male: bool, heat: Option<&str>) -> CategoryInfo {
        CategoryInfo {
            name: name.to_string(),
            discipline: Discipline::new(act_type, male),
            heat: heat.map(String::from),
        }
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn heat_1_names_the_offending_category() {
        let known = [
            info("EMU17", ActType::Single, true, None),
            info("EWU17", ActType::Single, false, None),
            info("EMU19", ActType::Single, true, Some("E EM 1")),
            info("KG", ActType::GroupSmall, false, None),
        ];
        assert_eq!(
            check("X", &names(&["EMU99"]), &known),
            Err(Problem::Unknown("EMU99".into()))
        );
        // Groups are heats like any other (#345), of one act type.
        assert_eq!(
            check("X", &names(&["KG"]), &known),
            Ok(Some(Discipline::new(ActType::GroupSmall, false)))
        );
        assert_eq!(
            check(
                "X",
                &names(&["KG", "GG"]),
                &[
                    known.to_vec(),
                    vec![info("GG", ActType::GroupLarge, false, None)]
                ]
                .concat()
            ),
            Err(Problem::OtherDiscipline {
                category: "GG".into(),
                first: "KG".into()
            })
        );
        assert_eq!(
            check("X", &names(&["EMU17", "EWU17"]), &known),
            Err(Problem::OtherDiscipline {
                category: "EWU17".into(),
                first: "EMU17".into()
            })
        );
        assert_eq!(
            check("X", &names(&["EMU19"]), &known),
            Err(Problem::InOtherHeat {
                category: "EMU19".into(),
                heat: "E EM 1".into()
            })
        );
        // Its own categories are fine.
        assert!(check("E EM 1", &names(&["EMU17", "EMU19"]), &known).is_ok());
    }

    #[test]
    fn junior_expert_heats_need_6_acts_for_a_vorlauf_others_5() {
        // U11–U15: Junior Expert.
        assert_eq!(default_threshold([Some(11.0), Some(13.0), Some(15.0)]), 6);
        // U17 and older, or without an upper age: Expert.
        assert_eq!(default_threshold([Some(15.0), Some(17.0)]), 5);
        assert_eq!(default_threshold([Some(13.0), None]), 5);
        // An empty heat gets the Expert default.
        assert_eq!(default_threshold([]), 5);
    }

    #[test]
    fn the_code_defaults_to_the_name() {
        assert_eq!(normalize_code(Some(" E  EM 1 "), "Einzel"), "E EM 1");
        assert_eq!(
            normalize_code(Some("  "), " Einzel  männlich "),
            "Einzel männlich"
        );
        assert_eq!(normalize_code(None, "Einzel"), "Einzel");
    }

    #[test]
    fn pairs_are_never_male() {
        assert_eq!(
            Discipline::new(ActType::Pair, true),
            Discipline::new(ActType::Pair, false)
        );
    }

    #[test]
    fn names_and_categories_are_trimmed() {
        assert_eq!(normalize_name("  E  EM 1 "), Ok("E EM 1".to_string()));
        assert!(normalize_name(" ").is_err());
        assert_eq!(
            normalize_categories(&names(&[" EMU11", "", "EMU13", "EMU11 "])),
            ["EMU11", "EMU13"]
        );
    }
}
