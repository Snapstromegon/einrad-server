//! Finals: start blocks with reserved slots for the best riders of the heats
//! ("1. Finalist (U11–U15)"), planned before the results are known.
//!
//! A final has slot groups, e.g. 4 × "U17–U21", then 4 × "U23+". They are
//! stored as one `final_slots` row per slot, which also holds the slot's live
//! state, like an act's. A final runs in at most one timeplan block
//! (`timeplan.final_name`); see `crate::timeplan`.
//!
//! Once the heats are judged, admins place acts of the final's categories
//! (`final_categories`) in its slots. The placement stays internal until it
//! is published (`finals.placement_published`): then the timeplan shows the
//! acts in their slots and the player plays their songs.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use uuid::Uuid;

use crate::act_type::ActType;

/// Most slots a final can have.
pub const MAX_SLOTS: i64 = 50;

/// A run of slots of a final, e.g. 5 × "U11–U15".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SlotGroup {
    /// Where these finalists come from, e.g. "U11–U15". May be empty.
    #[serde(default)]
    pub label: String,
    pub count: i64,
}

/// What admins set for a final, apart from its name.
#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct FinalFields {
    pub description: Option<String>,
    /// The discipline: act type and, for singles, male or female.
    pub act_type: ActType,
    #[serde(default)]
    pub is_single_male: bool,
    pub einfahrzeit_seconds: i64,
    pub act_duration_seconds: i64,
    pub judge_duration_seconds: i64,
    /// The reserved slots in start order, e.g. 5 × "U11–U15".
    pub slot_groups: Vec<SlotGroup>,
    /// The categories whose acts can be placed in the slots.
    #[serde(default)]
    pub categories: Vec<String>,
}

impl FinalFields {
    /// Trims the texts and checks the durations and slot groups. Only singles
    /// are male or female. The message is German.
    pub fn normalize(self) -> Result<Self, String> {
        if [
            self.einfahrzeit_seconds,
            self.act_duration_seconds,
            self.judge_duration_seconds,
        ]
        .iter()
        .any(|&seconds| seconds < 0)
        {
            return Err("Zeiten können nicht negativ sein.".to_string());
        }
        Ok(Self {
            description: self
                .description
                .map(|d| d.trim().to_string())
                .filter(|d| !d.is_empty()),
            is_single_male: self.is_single_male && self.act_type == ActType::Single,
            slot_groups: normalize_slot_groups(&self.slot_groups)?,
            categories: normalize_categories(&self.categories),
            ..self
        })
    }
}

/// Trims a final's name; fails with a German message if it's empty.
pub fn normalize_name(name: &str) -> Result<String, String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        Err("Ein Finale braucht einen Namen.".to_string())
    } else {
        Ok(name)
    }
}

/// The name of a reserved slot.
///
/// ```
/// use einrad_server::finals::finalist_label;
///
/// assert_eq!(finalist_label(3, "U17–U21"), "3. Finalist (U17–U21)");
/// assert_eq!(finalist_label(1, ""), "1. Finalist");
/// ```
pub fn finalist_label(number: i64, group_label: &str) -> String {
    if group_label.is_empty() {
        format!("{number}. Finalist")
    } else {
        format!("{number}. Finalist ({group_label})")
    }
}

/// Trims the labels and checks the slot groups of a final: at least one
/// slot, every group with at least one, at most [`MAX_SLOTS`] in all. The
/// message is German, for the admin UI.
pub fn normalize_slot_groups(groups: &[SlotGroup]) -> Result<Vec<SlotGroup>, String> {
    if groups.iter().any(|group| group.count < 1) {
        return Err("Jede Gruppe von Finalplätzen braucht mindestens einen Platz.".to_string());
    }
    let total: i64 = groups.iter().map(|group| group.count).sum();
    if total < 1 {
        return Err("Ein Finale braucht mindestens einen Finalplatz.".to_string());
    }
    if total > MAX_SLOTS {
        return Err(format!("Ein Finale hat höchstens {MAX_SLOTS} Finalplätze."));
    }
    Ok(groups
        .iter()
        .map(|group| SlotGroup {
            label: group.label.split_whitespace().collect::<Vec<_>>().join(" "),
            count: group.count,
        })
        .collect())
}

/// Trims the category names of a final and drops empty and repeated ones,
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

/// Replaces the slots of a final with new ones for `groups`, in order. A
/// slot keeps its placed act if the new slots have one with the same group
/// label and number. The caller checks that none of the old ones has started.
pub async fn replace_slots(
    conn: &mut SqliteConnection,
    final_name: &str,
    groups: &[SlotGroup],
) -> sqlx::Result<()> {
    let placed: HashMap<(String, i64), Uuid> = sqlx::query!(
        r#"
        SELECT group_label, number, act_id as "act_id!: Uuid"
        FROM final_slots
        WHERE final_name = ? AND act_id IS NOT NULL
        "#,
        final_name
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| ((row.group_label, row.number), row.act_id))
    .collect();
    sqlx::query!("DELETE FROM final_slots WHERE final_name = ?", final_name)
        .execute(&mut *conn)
        .await?;
    let mut position = 0_i64;
    for group in groups {
        for number in 1..=group.count {
            position += 1;
            let id = Uuid::now_v7();
            let act_id = placed.get(&(group.label.clone(), number));
            sqlx::query!(
                r#"
                INSERT INTO final_slots (id, final_name, position, group_label, number, act_id)
                VALUES (?, ?, ?, ?, ?, ?)
                "#,
                id,
                final_name,
                position,
                group.label,
                number,
                act_id,
            )
            .execute(&mut *conn)
            .await?;
        }
    }
    Ok(())
}

/// Sets the categories of a final, whose acts can be placed in its slots.
/// Acts already placed stay. The caller checks that the categories exist
/// (see [`check_categories`]).
pub async fn set_categories(
    conn: &mut SqliteConnection,
    final_name: &str,
    categories: &[String],
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM final_categories WHERE final_name = ?",
        final_name
    )
    .execute(&mut *conn)
    .await?;
    for category in categories {
        sqlx::query!(
            "INSERT INTO final_categories (final_name, category) VALUES (?, ?)",
            final_name,
            category
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Checks that the categories exist. The message is German.
pub async fn check_categories(
    conn: &mut SqliteConnection,
    categories: &[String],
) -> Result<(), String> {
    let known: Vec<String> = sqlx::query_scalar!(r#"SELECT name as "name!" FROM categories"#)
        .fetch_all(conn)
        .await
        .map_err(|e| e.to_string())?;
    let unknown: Vec<&str> = categories
        .iter()
        .filter(|category| !known.contains(category))
        .map(String::as_str)
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Diese Kategorien gibt es nicht: {}.",
            unknown.join(", ")
        ))
    }
}

/// Whether a slot of the final has started.
pub async fn has_started(conn: &mut SqliteConnection, final_name: &str) -> sqlx::Result<bool> {
    sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM final_slots WHERE final_name = ? AND started_at IS NOT NULL
        ) as "started!: bool"
        "#,
        final_name
    )
    .fetch_one(conn)
    .await
}

/// Checks that a final exists and runs in no timeplan block other than
/// `except` (the block being edited). The message is German.
pub async fn check_plannable(
    conn: &mut SqliteConnection,
    final_name: &str,
    except: Option<i64>,
) -> Result<(), String> {
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM finals WHERE name = ?) as "exists!: bool""#,
        final_name
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|e| e.to_string())?;
    if !exists {
        return Err(format!("Das Finale „{final_name}“ gibt es nicht."));
    }
    let planned = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM timeplan WHERE final_name = ? AND id IS NOT ?
        ) as "planned!: bool"
        "#,
        final_name,
        except
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|e| e.to_string())?;
    if planned {
        return Err(format!(
            "Das Finale „{final_name}“ ist schon im Zeitplan; jedes Finale läuft in einem Block."
        ));
    }
    Ok(())
}

/// The slot groups of slots in order: consecutive slots with the same group
/// label form a group.
///
/// ```
/// use einrad_server::finals::{SlotGroup, group_slots};
///
/// let groups = group_slots(["U17", "U17", "U23+"]);
/// assert_eq!(
///     groups,
///     [
///         SlotGroup { label: "U17".to_string(), count: 2 },
///         SlotGroup { label: "U23+".to_string(), count: 1 },
///     ]
/// );
/// ```
pub fn group_slots<'a>(labels: impl IntoIterator<Item = &'a str>) -> Vec<SlotGroup> {
    let mut groups: Vec<SlotGroup> = vec![];
    for label in labels {
        match groups.last_mut() {
            Some(group) if group.label == label => group.count += 1,
            _ => groups.push(SlotGroup {
                label: label.to_string(),
                count: 1,
            }),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(label: &str, count: i64) -> SlotGroup {
        SlotGroup {
            label: label.to_string(),
            count,
        }
    }

    #[test]
    fn slot_groups_are_trimmed_and_checked() {
        assert_eq!(
            normalize_slot_groups(&[group("  U11 –  U15 ", 5), group("", 1)]),
            Ok(vec![group("U11 – U15", 5), group("", 1)])
        );
        for groups in [vec![], vec![group("U11", 0)], vec![group("U11", -1)]] {
            assert!(normalize_slot_groups(&groups).is_err(), "{groups:?}");
        }
        assert!(normalize_slot_groups(&[group("A", 30), group("B", 20)]).is_ok());
        assert!(normalize_slot_groups(&[group("A", 30), group("B", 21)]).is_err());
    }

    #[test]
    fn categories_are_trimmed_and_deduplicated() {
        let categories = [" EMU11", "", "EMU13", "EMU11 "].map(String::from);
        assert_eq!(normalize_categories(&categories), ["EMU11", "EMU13"]);
    }
}
