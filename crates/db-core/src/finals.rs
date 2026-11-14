//! Finals: start blocks with reserved slots for the best riders of the heats
//! ("1. Finalist (JE EM)"), planned before the results are known.
//!
//! A final has slot groups, each filled from a heat (see `crate::heats`),
//! e.g. 4 × "E EM 1", then 4 × "E EM 2". They are stored as one
//! `final_slots` row per slot, which also holds the slot's live state, like
//! an act's. A final runs in at most one timeplan block
//! (`timeplan.final_id`); see `crate::timeplan`. Its categories are those
//! of its heats.
//!
//! Once the heats are judged, admins place acts of a slot's heat in it. The
//! placement stays internal until it is published
//! (`finals.placement_published`): then the timeplan shows the acts in their
//! slots and the player plays their songs.

use crate::ids::{ActId, FinalId, FinalSlotId, HeatId};

use serde::{Deserialize, Serialize};

use crate::act_type::ActType;

/// Most slots a final can have.
pub const MAX_SLOTS: i64 = 50;

/// A run of slots of a final filled from one heat, e.g. 5 × "JE EM", as the
/// competition file writes it: the heat by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SlotGroup {
    /// The name of the heat whose best acts fill these slots.
    pub heat: String,
    pub count: i64,
}

/// A run of slots as the admin API takes it: the heat by id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, utoipa::ToSchema)]
pub struct SlotGroupInput {
    /// The heat whose best acts fill these slots.
    pub heat_id: HeatId,
    pub count: i64,
}

/// A run of slots as the admin API returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct SlotGroupInfo {
    pub heat_id: HeatId,
    /// The heat's readable name.
    pub heat: String,
    /// The heat's short code.
    pub heat_code: String,
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
    pub slot_groups: Vec<SlotGroupInput>,
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
            slot_groups: check_counts(&self.slot_groups)?,
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

/// The name of a reserved slot, after its heat's code.
///
/// ```
/// use db_core::finals::finalist_label;
///
/// assert_eq!(finalist_label(3, "E EM 1"), "3. Finalist (E EM 1)");
/// ```
pub fn finalist_label(number: i64, heat_code: &str) -> String {
    format!("{number}. Finalist ({heat_code})")
}

/// Trims the heat names and checks the slot groups of a final: at least one
/// slot, every group with a heat and at least one slot, at most
/// [`MAX_SLOTS`] in all. The message is German, for the admin UI.
pub fn normalize_slot_groups(groups: &[SlotGroup]) -> Result<Vec<SlotGroup>, String> {
    if groups.iter().any(|group| group.heat.trim().is_empty()) {
        return Err("Jede Gruppe von Finalplätzen braucht eine Vorlaufklasse.".to_string());
    }
    check_total(groups.iter().map(|group| group.count))?;
    Ok(groups
        .iter()
        .map(|group| SlotGroup {
            heat: group.heat.split_whitespace().collect::<Vec<_>>().join(" "),
            count: group.count,
        })
        .collect())
}

/// Checks the slot counts of a final's groups: every group at least one
/// slot, at least one and at most [`MAX_SLOTS`] in all.
fn check_total(counts: impl Iterator<Item = i64> + Clone) -> Result<(), String> {
    if counts.clone().any(|count| count < 1) {
        return Err("Jede Gruppe von Finalplätzen braucht mindestens einen Platz.".to_string());
    }
    let total: i64 = counts.sum();
    if total < 1 {
        return Err("Ein Finale braucht mindestens einen Finalplatz.".to_string());
    }
    if total > MAX_SLOTS {
        return Err(format!("Ein Finale hat höchstens {MAX_SLOTS} Finalplätze."));
    }
    Ok(())
}

/// Checks the counts of the slot groups the admin API sends (the heats are
/// checked against the database by the backend).
pub fn check_counts(groups: &[SlotGroupInput]) -> Result<Vec<SlotGroupInput>, String> {
    check_total(groups.iter().map(|group| group.count))?;
    Ok(groups.to_vec())
}

/// The slot groups of slots in order, by heat id: consecutive slots of the
/// same heat form a group.
pub fn group_slot_ids(heats: impl IntoIterator<Item = HeatId>) -> Vec<SlotGroupInput> {
    let mut groups: Vec<SlotGroupInput> = vec![];
    for heat_id in heats {
        match groups.last_mut() {
            Some(group) if group.heat_id == heat_id => group.count += 1,
            _ => groups.push(SlotGroupInput { heat_id, count: 1 }),
        }
    }
    groups
}

/// The slot groups of slots in order, by heat id, name and code:
/// consecutive slots of the same heat form a group.
pub fn group_slot_infos<'a>(
    heats: impl IntoIterator<Item = (HeatId, &'a str, &'a str)>,
) -> Vec<SlotGroupInfo> {
    let mut groups: Vec<SlotGroupInfo> = vec![];
    for (id, name, code) in heats {
        match groups.last_mut() {
            Some(group) if group.heat_id == id => group.count += 1,
            _ => groups.push(SlotGroupInfo {
                heat_id: id,
                heat: name.to_string(),
                heat_code: code.to_string(),
                count: 1,
            }),
        }
    }
    groups
}

/// The slot groups of slots in order, by heat name as the competition file
/// has them: consecutive slots of the same heat form a group.
///
/// ```
/// use db_core::finals::{SlotGroup, group_slots};
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

/// A reserved slot of a final.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct FinalSlot {
    pub id: FinalSlotId,
    /// "3. Finalist (E EM 1)".
    pub label: String,
    /// The heat whose acts fill the slot, by id.
    pub heat_id: HeatId,
    /// The heat's short code, as the label has it.
    pub group_label: String,
    /// Counts within the group.
    pub number: i64,
    /// The placed act. Before the placement is published, only for who
    /// manages results.
    pub act_id: Option<ActId>,
    pub started: bool,
}

/// A final as the API lists it.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Final {
    pub id: FinalId,
    /// The readable name, unique, e.g. "Finale Einzel männlich Expert".
    pub name: String,
    /// The short code, unique, e.g. "E EM".
    pub code: String,
    pub description: Option<String>,
    pub act_type: ActType,
    pub is_single_male: bool,
    pub order: i64,
    pub einfahrzeit_seconds: i64,
    pub act_duration_seconds: i64,
    pub judge_duration_seconds: i64,
    /// The reserved slots in start order, e.g. 5 × "JE EM".
    pub slot_groups: Vec<SlotGroupInfo>,
    /// The categories of its heats, whose acts can be placed in the slots.
    pub categories: Vec<String>,
    /// The slots in start order, with their placed acts.
    pub slots: Vec<FinalSlot>,
    /// Whether the placement is published: the timeplan shows the acts.
    pub placement_published: bool,
    /// Whether a slot has started: the slot groups can't change any more.
    pub started: bool,
    /// The timeplan block it runs in, if it is planned.
    pub timeplan_id: Option<i64>,
}

/// A final to add; name and code are normalized.
#[derive(Debug, Clone)]
pub struct NewFinal {
    pub name: String,
    pub code: String,
    pub fields: FinalFields,
}

/// What an edit of a final sets; the name is normalized.
#[derive(Debug, Clone)]
pub struct FinalChange {
    pub id: FinalId,
    pub new_name: String,
    /// The new normalized code; without, it stays.
    pub new_code: Option<String>,
    pub fields: FinalFields,
}

/// A change of the act placed in a slot.
#[derive(Debug, Clone, Copy)]
pub struct SlotPlacement {
    pub slot_id: FinalSlotId,
    /// `None` empties the slot.
    pub act_id: Option<ActId>,
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
