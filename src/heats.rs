//! Heats (Vorläufe, #338): a named group of categories of one discipline
//! whose acts are ranked together to fill a final's finalist slots.
//!
//! HEAT-1: all categories of a heat share one act type and, for singles,
//! gender; a category belongs to at most one heat (`categories.heat`), group
//! categories to none: they are their own final. A heat may be empty until
//! the admin assigns its categories. Its discipline follows from its
//! categories; an empty heat has none and fits any final.
//!
//! A final's slot groups refer to heats (`final_slots.heat`, see
//! `crate::finals`); the final's categories are those of its heats. Clubs
//! never see or pick a heat: it follows from the category.

use sqlx::SqliteConnection;

use crate::act_type::ActType;

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

    fn is_group(self) -> bool {
        matches!(self.act_type, ActType::GroupSmall | ActType::GroupLarge)
    }
}

/// A category as far as heats care.
#[derive(Debug, Clone, PartialEq)]
pub struct CategoryInfo {
    pub name: String,
    pub discipline: Discipline,
    /// The heat it belongs to, if any.
    pub heat: Option<String>,
}

/// Why categories can't form a heat (HEAT-1), naming the offender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// No category of that name.
    Unknown(String),
    /// A group category: groups are their own final.
    Group(String),
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
            Problem::Group(category) => format!(
                "{category} ist eine Gruppenkategorie; Gruppen sind ihr eigenes Finale und gehören zu keinem Vorlauf."
            ),
            Problem::OtherDiscipline { category, first } => format!(
                "{category} hat einen anderen Typ oder ein anderes Geschlecht als {first}; alle Kategorien eines Vorlaufs teilen beides."
            ),
            Problem::InOtherHeat { category, heat } => {
                format!("{category} gehört schon zum Vorlauf „{heat}“.")
            }
        }
    }

    /// The problem in English, for the competition file.
    pub fn english(&self) -> String {
        match self {
            Problem::Unknown(category) => {
                format!("category {category} isn't a category of the file")
            }
            Problem::Group(category) => format!(
                "{category} is a group category; groups are their own final and belong to no heat"
            ),
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
/// use einrad_server::act_type::ActType;
/// use einrad_server::heats::{CategoryInfo, Discipline, Problem, check};
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
        if category.discipline.is_group() {
            return Err(Problem::Group(name.clone()));
        }
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

/// Trims a heat's name; fails with a German message if it's empty.
pub fn normalize_name(name: &str) -> Result<String, String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        Err("Ein Vorlauf braucht einen Namen.".to_string())
    } else {
        Ok(name)
    }
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

/// The categories as far as heats care.
pub async fn categories(conn: &mut SqliteConnection) -> sqlx::Result<Vec<CategoryInfo>> {
    Ok(sqlx::query!(
        r#"
        SELECT name as "name!", act_type as "act_type: ActType", is_single_male, heat
        FROM categories
        ORDER BY "order", name
        "#
    )
    .fetch_all(conn)
    .await?
    .into_iter()
    .map(|row| CategoryInfo {
        name: row.name,
        discipline: Discipline::new(row.act_type, row.is_single_male),
        heat: row.heat,
    })
    .collect())
}

/// The discipline of a heat, from its categories; `None` if it has none.
pub async fn discipline(
    conn: &mut SqliteConnection,
    heat: &str,
) -> sqlx::Result<Option<Discipline>> {
    Ok(sqlx::query!(
        r#"
        SELECT act_type as "act_type: ActType", is_single_male
        FROM categories WHERE heat = ?
        ORDER BY "order" LIMIT 1
        "#,
        heat
    )
    .fetch_optional(conn)
    .await?
    .map(|row| Discipline::new(row.act_type, row.is_single_male)))
}

/// The finals whose slots a heat fills, with their disciplines, by name.
pub async fn finals_of(
    conn: &mut SqliteConnection,
    heat: &str,
) -> sqlx::Result<Vec<(String, Discipline)>> {
    Ok(sqlx::query!(
        r#"
        SELECT DISTINCT finals.name as "name!", act_type as "act_type: ActType", is_single_male
        FROM finals JOIN final_slots ON final_slots.final_name = finals.name
        WHERE final_slots.heat = ?
        ORDER BY finals.name
        "#,
        heat
    )
    .fetch_all(conn)
    .await?
    .into_iter()
    .map(|row| (row.name, Discipline::new(row.act_type, row.is_single_male)))
    .collect())
}

/// Sets the categories of a heat: the ones named join it, the others leave
/// it. The caller checks them first (see [`check`]).
pub async fn set_categories(
    conn: &mut SqliteConnection,
    heat: &str,
    categories: &[String],
) -> sqlx::Result<()> {
    sqlx::query!("UPDATE categories SET heat = NULL WHERE heat = ?", heat)
        .execute(&mut *conn)
        .await?;
    for category in categories {
        sqlx::query!(
            "UPDATE categories SET heat = ? WHERE name = ?",
            heat,
            category
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Checks a heat's new categories (HEAT-1) and that they keep the
/// discipline of the finals it fills. The message is German.
pub async fn check_categories(
    conn: &mut SqliteConnection,
    heat: &str,
    categories: &[String],
) -> Result<(), String> {
    let known = self::categories(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    let Some(discipline) = check(heat, categories, &known).map_err(|p| p.german())? else {
        return Ok(());
    };
    let finals = finals_of(conn, heat).await.map_err(|e| e.to_string())?;
    if let Some((name, _)) = finals.iter().find(|(_, d)| *d != discipline) {
        return Err(format!(
            "Der Vorlauf füllt Finalplätze im Finale „{name}“; seine Kategorien brauchen dessen Typ und Geschlecht."
        ));
    }
    Ok(())
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
        assert_eq!(
            check("X", &names(&["KG"]), &known),
            Err(Problem::Group("KG".into()))
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
