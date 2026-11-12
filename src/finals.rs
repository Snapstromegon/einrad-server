//! Finals: start blocks with reserved slots for the best riders of the heats
//! ("1. Finalist (JE EM)"), planned before the results are known.
//!
//! A final has slot groups, each filled from a heat (see `crate::heats`),
//! e.g. 4 × "E EM 1", then 4 × "E EM 2". They are stored as one
//! `final_slots` row per slot, which also holds the slot's live state, like
//! an act's. A final runs in at most one timeplan block
//! (`timeplan.final_name`); see `crate::timeplan`. Its categories are those
//! of its heats.
//!
//! Once the heats are judged, admins place acts of a slot's heat in it. The
//! placement stays internal until it is published
//! (`finals.placement_published`): then the timeplan shows the acts in their
//! slots and the player plays their songs.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use uuid::Uuid;

use crate::{act_type::ActType, heats::Discipline};

/// Most slots a final can have.
pub const MAX_SLOTS: i64 = 50;

/// A run of slots of a final filled from one heat, e.g. 5 × "JE EM".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SlotGroup {
    /// The heat whose best acts fill these slots.
    pub heat: String,
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
    /// The reserved slots in start order, e.g. 5 × "JE EM".
    pub slot_groups: Vec<SlotGroup>,
}

impl FinalFields {
    /// Trims the texts and checks the durations and slot groups. Only singles
    /// are male or female. The message is
    /// German.
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

/// The name of a reserved slot, after its heat.
///
/// ```
/// use einrad_server::finals::finalist_label;
///
/// assert_eq!(finalist_label(3, "E EM 1"), "3. Finalist (E EM 1)");
/// ```
pub fn finalist_label(number: i64, heat: &str) -> String {
    format!("{number}. Finalist ({heat})")
}

/// Trims the heat names and checks the slot groups of a final: at least one
/// slot, every group with a heat and at least one slot, at most
/// [`MAX_SLOTS`] in all. The message is German, for the admin UI.
pub fn normalize_slot_groups(groups: &[SlotGroup]) -> Result<Vec<SlotGroup>, String> {
    if groups.iter().any(|group| group.heat.trim().is_empty()) {
        return Err("Jede Gruppe von Finalplätzen braucht eine Vorlaufklasse.".to_string());
    }
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
            heat: group.heat.split_whitespace().collect::<Vec<_>>().join(" "),
            count: group.count,
        })
        .collect())
}

/// Replaces the slots of a final with new ones for `groups`, in order. A
/// slot keeps its placed act if the new slots have one with the same heat
/// and number. The caller checks that none of the old ones has started and
/// that the heats exist.
pub async fn replace_slots(
    conn: &mut SqliteConnection,
    final_name: &str,
    groups: &[SlotGroup],
) -> sqlx::Result<()> {
    let placed: HashMap<(String, i64), Uuid> = sqlx::query!(
        r#"
        SELECT heat as "heat!", number, act_id as "act_id!: Uuid"
        FROM final_slots
        WHERE final_name = ? AND act_id IS NOT NULL
        "#,
        final_name
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| ((row.heat, row.number), row.act_id))
    .collect();
    sqlx::query!("DELETE FROM final_slots WHERE final_name = ?", final_name)
        .execute(&mut *conn)
        .await?;
    let mut position = 0_i64;
    for group in groups {
        for number in 1..=group.count {
            position += 1;
            let id = Uuid::now_v7();
            let act_id = placed.get(&(group.heat.clone(), number));
            sqlx::query!(
                r#"
                INSERT INTO final_slots (id, final_name, position, heat, number, act_id)
                VALUES (?, ?, ?, ?, ?, ?)
                "#,
                id,
                final_name,
                position,
                group.heat,
                number,
                act_id,
            )
            .execute(&mut *conn)
            .await?;
        }
    }
    Ok(())
}

/// Checks that the heats of the final `final_name`'s slot groups exist,
/// share its discipline (an empty heat fits any) and fill no other final.
/// The message is German.
pub async fn check_heats(
    conn: &mut SqliteConnection,
    final_name: &str,
    act_type: ActType,
    is_single_male: bool,
    groups: &[SlotGroup],
) -> Result<(), String> {
    let discipline = Discipline::new(act_type, is_single_male);
    for group in groups {
        let exists = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM heats WHERE name = ?) as "exists!: bool""#,
            group.heat
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
        if !exists {
            return Err(format!("Die Vorlaufklasse „{}“ gibt es nicht.", group.heat));
        }
        let heat = crate::heats::discipline(&mut *conn, &group.heat)
            .await
            .map_err(|e| e.to_string())?;
        if heat.is_some_and(|heat| heat != discipline) {
            return Err(format!(
                "Die Vorlaufklasse „{}“ hat einen anderen Typ oder ein anderes Geschlecht als das Finale.",
                group.heat
            ));
        }
        let other = sqlx::query_scalar!(
            "SELECT final_name FROM final_slots WHERE heat = ? AND final_name != ? LIMIT 1",
            group.heat,
            final_name
        )
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
        if let Some(other) = other {
            return Err(format!(
                "Die Vorlaufklasse „{}“ füllt schon Finalplätze im Finale „{other}“; eine Vorlaufklasse füllt höchstens ein Finale.",
                group.heat
            ));
        }
    }
    Ok(())
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

/// The slot groups of slots in order: consecutive slots of the same heat
/// form a group.
///
/// ```
/// use einrad_server::finals::{SlotGroup, group_slots};
///
/// let groups = group_slots(["E EM 1", "E EM 1", "E EM 2"]);
/// assert_eq!(
///     groups,
///     [
///         SlotGroup { heat: "E EM 1".to_string(), count: 2 },
///         SlotGroup { heat: "E EM 2".to_string(), count: 1 },
///     ]
/// );
/// ```
pub fn group_slots<'a>(heats: impl IntoIterator<Item = &'a str>) -> Vec<SlotGroup> {
    let mut groups: Vec<SlotGroup> = vec![];
    for heat in heats {
        match groups.last_mut() {
            Some(group) if group.heat == heat => group.count += 1,
            _ => groups.push(SlotGroup {
                heat: heat.to_string(),
                count: 1,
            }),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(heat: &str, count: i64) -> SlotGroup {
        SlotGroup {
            heat: heat.to_string(),
            count,
        }
    }

    #[test]
    fn slot_groups_are_trimmed_and_checked() {
        assert_eq!(
            normalize_slot_groups(&[group("  JE  EM ", 5), group("E EM", 1)]),
            Ok(vec![group("JE EM", 5), group("E EM", 1)])
        );
        for groups in [vec![], vec![group("U11", 0)], vec![group("U11", -1)]] {
            assert!(normalize_slot_groups(&groups).is_err(), "{groups:?}");
        }
        assert!(normalize_slot_groups(&[group("A", 30), group("B", 20)]).is_ok());
        assert!(normalize_slot_groups(&[group("A", 30), group("B", 21)]).is_err());
    }
}
