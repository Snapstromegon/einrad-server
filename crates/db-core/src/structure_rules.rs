//! The rules of categories, heats and finals as pure functions (#238): free
//! names and codes, which categories form a heat, when a heat or final can be
//! deleted, skipped or changed, and the placement of acts in final slots.
//!
//! A backend loads the facts (plain structs here), calls the decision and
//! writes. Every refusal carries its German message.

use std::collections::{HashMap, HashSet};

use crate::{
    clubs::RegistrationError,
    finals::{SlotGroupInput, SlotPlacement, finalist_label, group_slot_ids},
    heats::{self, CategoryInfo, Discipline},
    ids::{ActId, CategoryId, FinalSlotId, HeatId},
};

fn rejected(message: impl Into<String>) -> RegistrationError {
    RegistrationError::Rejected(message.into())
}

fn conflict(message: impl Into<String>) -> RegistrationError {
    RegistrationError::Conflict(message.into())
}

/// The name and code of an existing heat or final.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameCode {
    pub name: String,
    pub code: String,
}

fn name_taken<'a>(
    others: impl IntoIterator<Item = &'a NameCode>,
    name: &str,
    code: &str,
) -> (bool, bool) {
    let mut name_taken = false;
    let mut code_taken = false;
    for other in others {
        name_taken |= other.name == name;
        code_taken |= other.code == code;
    }
    (name_taken, code_taken)
}

/// Checks that no other heat has the name or the code; `others` are all
/// heats except the one being edited. The name is checked first.
pub fn check_heat_free(
    others: &[NameCode],
    name: &str,
    code: &str,
) -> Result<(), RegistrationError> {
    let (name_taken, code_taken) = name_taken(others, name, code);
    if name_taken {
        return Err(conflict(format!(
            "Eine Vorlaufklasse „{name}“ gibt es schon."
        )));
    }
    if code_taken {
        return Err(conflict(format!(
            "Eine Vorlaufklasse mit dem Kürzel „{code}“ gibt es schon."
        )));
    }
    Ok(())
}

/// Checks that no other final has the name or the code; `others` are all
/// finals except the one being edited. The name is checked first.
pub fn check_final_free(
    others: &[NameCode],
    name: &str,
    code: &str,
) -> Result<(), RegistrationError> {
    let (name_taken, code_taken) = name_taken(others, name, code);
    if name_taken {
        return Err(conflict(format!("Ein Finale „{name}“ gibt es schon.")));
    }
    if code_taken {
        return Err(conflict(format!(
            "Ein Finale mit dem Kürzel „{code}“ gibt es schon."
        )));
    }
    Ok(())
}

/// "There is no such heat" (the 400 the admin UI shows).
pub fn heat_missing_message() -> String {
    "Die Vorlaufklasse gibt es nicht.".to_string()
}

/// The names of the categories with these IDs, in the order given and
/// without repeats; `known` are all categories by id. An ID no category has
/// is a 404 with a German message naming it.
pub fn category_names(
    ids: &[CategoryId],
    known: &[(CategoryId, String)],
) -> Result<Vec<String>, RegistrationError> {
    let mut names: Vec<String> = vec![];
    for id in ids {
        let (_, name) = known.iter().find(|(known, _)| known == id).ok_or_else(|| {
            RegistrationError::NotFoundMessage(format!("Die Kategorie {id} gibt es nicht (mehr)."))
        })?;
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    Ok(names)
}

/// What the check of a heat's new categories needs.
#[derive(Debug, Clone)]
pub struct HeatCategoryFacts {
    /// The heat's name; `None` if there is no such heat.
    pub heat: Option<String>,
    /// All categories, with the heat each belongs to.
    pub known: Vec<CategoryInfo>,
    /// The finals whose slots the heat fills, by name, with their discipline.
    pub finals: Vec<(String, Discipline)>,
}

/// Checks the new categories of a heat (HEAT-1) and that they keep the
/// discipline of the finals the heat fills.
pub fn check_heat_categories(
    facts: &HeatCategoryFacts,
    categories: &[String],
) -> Result<(), RegistrationError> {
    let heat = facts
        .heat
        .as_deref()
        .ok_or_else(|| rejected(heat_missing_message()))?;
    let Some(discipline) =
        heats::check(heat, categories, &facts.known).map_err(|p| rejected(p.german()))?
    else {
        return Ok(());
    };
    if let Some((name, _)) = facts.finals.iter().find(|(_, d)| *d != discipline) {
        return Err(rejected(format!(
            "Die Vorlaufklasse füllt Finalplätze im Finale „{name}“; ihre Kategorien brauchen dessen Typ und Geschlecht."
        )));
    }
    Ok(())
}

/// A new heat of groups starts with its Vorlauf skipped (#345); the
/// organizer may restore it.
pub fn heat_starts_skipped(discipline: Option<Discipline>) -> bool {
    discipline.is_some_and(Discipline::is_group)
}

/// Refuses (409) to delete a heat that fills slots of finals (`finals`, by
/// name) or whose jury unit has judges (`jury_unit_use`, registrations plus
/// assignments).
pub fn check_heat_deletable(
    finals: &[String],
    jury_unit_use: i64,
) -> Result<(), RegistrationError> {
    if !finals.is_empty() {
        let names: Vec<String> = finals.iter().map(|name| format!("„{name}“")).collect();
        return Err(conflict(format!(
            "Die Vorlaufklasse füllt Finalplätze in {} {}; bitte sie dort zuerst durch einen anderen ersetzen.",
            if names.len() == 1 {
                "dem Finale"
            } else {
                "den Finals"
            },
            names.join(", ")
        )));
    }
    check_jury_unit_unused(jury_unit_use)
}

/// Refuses (409) to delete a heat or final while judges are registered or
/// assigned to its jury unit (`use_count`); the jury group may promote it.
pub fn check_jury_unit_unused(use_count: i64) -> Result<(), RegistrationError> {
    if use_count > 0 {
        return Err(conflict(
            "Für die Jury dieser Vorlaufklasse oder dieses Finales sind JurorInnen gemeldet oder eingeteilt. Erst das ändern, dann löschen.",
        ));
    }
    Ok(())
}

/// Whether the Vorlauf of a heat changes: `Ok(false)` if it already is as
/// wanted; refused once an act of the heat or its final has started.
pub fn check_set_heat_skip(
    heat_name: &str,
    current: bool,
    wanted: bool,
    started: bool,
) -> Result<bool, RegistrationError> {
    if current == wanted {
        return Ok(false);
    }
    if started {
        return Err(rejected(format!(
            "In der Vorlaufklasse „{heat_name}“ oder ihrem Finale hat schon eine Kür begonnen; ob ihr Vorlauf entfällt, lässt sich nicht mehr ändern."
        )));
    }
    Ok(true)
}

/// A heat of a final's slot group, as loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotHeatFacts {
    /// The heat's name; `None` if there is no such heat.
    pub name: Option<String>,
    /// Its discipline; `None` while it has no categories.
    pub discipline: Option<Discipline>,
    /// A final other than the one edited whose slots it fills.
    pub other_final: Option<String>,
}

/// Checks that the heats of a final's slot groups (in group order) exist,
/// share its discipline (an empty heat fits any) and fill no other final.
pub fn check_final_heats(
    discipline: Discipline,
    heats: &[SlotHeatFacts],
) -> Result<(), RegistrationError> {
    for facts in heats {
        let Some(heat) = &facts.name else {
            return Err(rejected(heat_missing_message()));
        };
        if facts.discipline.is_some_and(|heat| heat != discipline) {
            return Err(rejected(format!(
                "Die Vorlaufklasse „{heat}“ hat einen anderen Typ oder ein anderes Geschlecht als das Finale."
            )));
        }
        if let Some(other) = &facts.other_final {
            return Err(rejected(format!(
                "Die Vorlaufklasse „{heat}“ füllt schon Finalplätze im Finale „{other}“; eine Vorlaufklasse füllt höchstens ein Finale."
            )));
        }
    }
    Ok(())
}

/// Whether the slots of a final are replaced by an edit: only when the slot
/// groups differ from the `current` heats of its slots in order, and then
/// not once the final has started.
pub fn check_slot_groups_change(
    current: impl IntoIterator<Item = HeatId>,
    new: &[SlotGroupInput],
    started: bool,
) -> Result<bool, RegistrationError> {
    if group_slot_ids(current) == new {
        return Ok(false);
    }
    if started {
        return Err(rejected(
            "Das Finale hat schon begonnen, seine Finalplätze bleiben, wie sie sind.",
        ));
    }
    Ok(true)
}

/// An act as the "has it started" decision sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActRunFacts {
    pub started: bool,
    pub withdrawn: bool,
}

impl ActRunFacts {
    /// A withdrawn act doesn't count, even if it had started.
    #[must_use]
    pub fn counts_as_started(&self) -> bool {
        self.started && !self.withdrawn
    }
}

fn any_counts(acts: &[ActRunFacts]) -> bool {
    acts.iter().any(ActRunFacts::counts_as_started)
}

/// What a final's run shows: whether its slots and its timeplan blocks
/// started.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FinalRunFacts {
    /// Per slot of the final(s): has it started.
    pub slots_started: Vec<bool>,
    /// Per timeplan block running the final(s): has it started.
    pub blocks_started: Vec<bool>,
}

impl FinalRunFacts {
    /// A slot or the block of the final has started.
    #[must_use]
    pub fn has_started(&self) -> bool {
        self.slots_started
            .iter()
            .chain(&self.blocks_started)
            .any(|&s| s)
    }
}

/// A heat has started when one of its acts has (a withdrawn one doesn't
/// count) or when a final it feeds has (a slot, or its block). Then its
/// Vorlauf can no longer be skipped or restored.
#[must_use]
pub fn heat_has_started(heat_acts: &[ActRunFacts], finals: &FinalRunFacts) -> bool {
    any_counts(heat_acts) || finals.has_started()
}

/// An act of a heat that feeds a final.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeederActFacts {
    /// Its heat skips the Vorlauf.
    pub heat_skipped: bool,
    pub act: ActRunFacts,
}

/// A final has started when a slot or its block has, or when an act of a
/// skipped heat that fills it has (a withdrawn one doesn't count; acts of a
/// heat with a Vorlauf run there, not in the final). A final that runs only
/// direct acts (group finals, skipped heats) has no started slot.
#[must_use]
pub fn final_has_started(finals: &FinalRunFacts, feeder_acts: &[FeederActFacts]) -> bool {
    finals.has_started()
        || feeder_acts
            .iter()
            .any(|a| a.heat_skipped && a.act.counts_as_started())
}

/// Refuses (409) to delete a final that has started or whose jury unit has
/// judges.
pub fn check_final_deletable(started: bool, jury_unit_use: i64) -> Result<(), RegistrationError> {
    if started {
        return Err(conflict(
            "Das Finale hat schon begonnen und kann nicht mehr gelöscht werden.",
        ));
    }
    check_jury_unit_unused(jury_unit_use)
}

/// Refuses to withdraw the published placement of a final that has started.
pub fn check_final_published(published: bool, started: bool) -> Result<(), RegistrationError> {
    if !published && started {
        return Err(rejected(
            "Das Finale hat schon begonnen, seine Platzierung bleibt veröffentlicht.",
        ));
    }
    Ok(())
}

/// What the timeplan knows about a final a block is to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalPlanFacts {
    /// The final's name; `None` if there is no such final.
    pub name: Option<String>,
    /// Another block (not the one edited) already runs it.
    pub planned_elsewhere: bool,
}

/// Checks that a final exists and runs in no timeplan block other than the
/// one being edited.
pub fn check_final_plannable(facts: &FinalPlanFacts) -> Result<(), RegistrationError> {
    let Some(name) = &facts.name else {
        return Err(rejected("Das Finale gibt es nicht."));
    };
    if facts.planned_elsewhere {
        return Err(rejected(format!(
            "Das Finale „{name}“ ist schon im Zeitplan; jedes Finale läuft in einem Block."
        )));
    }
    Ok(())
}

/// A slot of the final whose placement changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementSlot {
    pub id: FinalSlotId,
    pub heat_id: HeatId,
    pub heat_name: String,
    pub heat_code: String,
    pub number: i64,
    pub act_id: Option<ActId>,
    pub started: bool,
}

/// An act to place, as loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacedAct {
    /// There is no such act.
    Missing,
    /// Withdrawn or without category: such acts don't start.
    Unavailable,
    /// Starts in this category.
    InCategory(String),
}

/// Checks a placement of acts in the slots of a final as a whole and gives
/// the slots that change. `heats` is the heat of each category by name;
/// `acts` the acts named by the changes.
pub fn plan_placement(
    slots: &[PlacementSlot],
    changes: &[SlotPlacement],
    acts: &HashMap<ActId, PlacedAct>,
    heats: &HashMap<String, HeatId>,
) -> Result<Vec<(FinalSlotId, Option<ActId>)>, RegistrationError> {
    let mut placement: HashMap<FinalSlotId, Option<ActId>> =
        slots.iter().map(|slot| (slot.id, slot.act_id)).collect();
    let mut changed = vec![];
    for change in changes {
        let Some(slot) = slots.iter().find(|slot| slot.id == change.slot_id) else {
            return Err(rejected(
                "Diesen Finalplatz gibt es in diesem Finale nicht.",
            ));
        };
        if slot.act_id == change.act_id {
            continue;
        }
        let label = finalist_label(slot.number, &slot.heat_code);
        if slot.started {
            return Err(rejected(format!(
                "„{label}“ hat schon begonnen, die Kür bleibt."
            )));
        }
        if let Some(act_id) = change.act_id {
            match acts.get(&act_id).unwrap_or(&PlacedAct::Missing) {
                PlacedAct::Missing => {
                    return Err(rejected(format!("Die Kür für „{label}“ gibt es nicht.")));
                }
                PlacedAct::Unavailable => {
                    return Err(rejected(format!(
                        "Die Kür für „{label}“ ist zurückgezogen oder unvollständig."
                    )));
                }
                PlacedAct::InCategory(category) if heats.get(category) != Some(&slot.heat_id) => {
                    return Err(rejected(format!(
                        "Die Kür für „{label}“ startet in {category}, einer Kategorie, die nicht zur Vorlaufklasse „{}“ gehört.",
                        slot.heat_name
                    )));
                }
                PlacedAct::InCategory(_) => {}
            }
        }
        placement.insert(slot.id, change.act_id);
        changed.push((slot.id, change.act_id));
    }
    let mut seen = HashSet::new();
    if placement
        .values()
        .flatten()
        .any(|act_id| !seen.insert(*act_id))
    {
        return Err(rejected(
            "Eine Kür kann nur einen Finalplatz eines Finales bekommen.",
        ));
    }
    Ok(changed)
}

/// A category in a heat keeps its act type and gender (HEAT-1): the heat's
/// other categories and its finals share them. `in_heat` is the category's
/// name, the heat's name and the discipline it has now, if it is in a heat.
pub fn check_category_keeps_discipline(
    in_heat: Option<(&str, &str, Discipline)>,
    new: Discipline,
) -> Result<(), RegistrationError> {
    match in_heat {
        Some((category, heat, now)) if now != new => Err(rejected(format!(
            "{category} gehört zur Vorlaufklasse „{heat}“: Typ und Geschlecht bleiben, solange die Kategorie dort ist."
        ))),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act_type::ActType;

    fn message<T: std::fmt::Debug>(result: Result<T, RegistrationError>) -> String {
        match result.unwrap_err() {
            RegistrationError::Rejected(m)
            | RegistrationError::Conflict(m)
            | RegistrationError::NotFoundMessage(m) => m,
            other => panic!("no message: {other:?}"),
        }
    }

    #[test]
    fn what_counts_as_started() {
        let run = |started, withdrawn| ActRunFacts { started, withdrawn };
        let feeder = |heat_skipped, started, withdrawn| FeederActFacts {
            heat_skipped,
            act: run(started, withdrawn),
        };
        let nothing = FinalRunFacts::default();
        let slot = FinalRunFacts {
            slots_started: vec![false, true],
            blocks_started: vec![],
        };
        let block = FinalRunFacts {
            slots_started: vec![false],
            blocks_started: vec![true],
        };
        // Nothing ran.
        assert!(!heat_has_started(&[run(false, false)], &nothing));
        assert!(!final_has_started(&nothing, &[feeder(true, false, false)]));
        // A started act starts the heat; a withdrawn one doesn't.
        assert!(heat_has_started(
            &[run(false, false), run(true, false)],
            &nothing
        ));
        assert!(!heat_has_started(&[run(true, true)], &nothing));
        // The final's slots or block start the heat and the final.
        assert!(heat_has_started(&[], &slot));
        assert!(heat_has_started(&[], &block));
        assert!(final_has_started(&slot, &[]));
        assert!(final_has_started(&block, &[]));
        // Acts of a skipped heat start the final, unless withdrawn.
        assert!(final_has_started(&nothing, &[feeder(true, true, false)]));
        assert!(!final_has_started(&nothing, &[feeder(true, true, true)]));
        // Acts of a heat with a Vorlauf start that, not the final.
        assert!(!final_has_started(&nothing, &[feeder(false, true, false)]));
    }

    fn single(male: bool) -> Discipline {
        Discipline::new(ActType::Single, male)
    }

    fn nc(name: &str, code: &str) -> NameCode {
        NameCode {
            name: name.into(),
            code: code.into(),
        }
    }

    #[test]
    fn names_and_codes_of_heats_and_finals_are_free() {
        let others = [nc("Vorlauf A", "A"), nc("Vorlauf B", "B")];
        assert!(check_heat_free(&others, "Vorlauf C", "C").is_ok());
        assert!(check_heat_free(&[], "Vorlauf A", "A").is_ok());
        assert!(matches!(
            check_heat_free(&others, "Vorlauf A", "X"),
            Err(RegistrationError::Conflict(_))
        ));
        assert_eq!(
            message(check_heat_free(&others, "Vorlauf A", "B")),
            "Eine Vorlaufklasse „Vorlauf A“ gibt es schon."
        );
        assert_eq!(
            message(check_heat_free(&others, "Neu", "B")),
            "Eine Vorlaufklasse mit dem Kürzel „B“ gibt es schon."
        );
        assert_eq!(
            message(check_final_free(&others, "Vorlauf B", "Z")),
            "Ein Finale „Vorlauf B“ gibt es schon."
        );
        assert_eq!(
            message(check_final_free(&others, "Neu", "A")),
            "Ein Finale mit dem Kürzel „A“ gibt es schon."
        );
    }

    #[test]
    fn category_ids_resolve_in_order_without_repeats() {
        let (a, b) = (CategoryId::new(), CategoryId::new());
        let known = [(a, "EM".to_string()), (b, "EF".to_string())];
        assert_eq!(category_names(&[b, a, b], &known).unwrap(), ["EF", "EM"]);
        let gone = CategoryId::new();
        match category_names(&[a, gone], &known) {
            Err(RegistrationError::NotFoundMessage(m)) => {
                assert_eq!(m, format!("Die Kategorie {gone} gibt es nicht (mehr)."));
            }
            other => panic!("{other:?}"),
        }
    }

    fn category(name: &str, discipline: Discipline, heat: Option<&str>) -> CategoryInfo {
        CategoryInfo {
            name: name.into(),
            discipline,
            heat: heat.map(str::to_string),
        }
    }

    #[test]
    fn heat_categories_share_a_discipline_and_keep_the_finals() {
        let mut facts = HeatCategoryFacts {
            heat: Some("Vorlauf EM".into()),
            known: vec![
                category("EM", single(true), None),
                category("EM2", single(true), Some("Vorlauf EM")),
                category("EF", single(false), None),
                category("X", single(true), Some("Anderes")),
            ],
            finals: vec![],
        };
        let names = |n: &[&str]| n.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert!(check_heat_categories(&facts, &names(&["EM", "EM2"])).is_ok());
        assert!(check_heat_categories(&facts, &[]).is_ok());
        assert!(message(check_heat_categories(&facts, &names(&["EM", "EF"]))).contains("EF"));
        assert!(message(check_heat_categories(&facts, &names(&["X"]))).contains("Anderes"));
        facts.finals = vec![("Finale EF".into(), single(false))];
        assert!(
            message(check_heat_categories(&facts, &names(&["EM"])))
                .contains("im Finale „Finale EF“")
        );
        assert!(check_heat_categories(&facts, &names(&["EF"])).is_ok());
        facts.heat = None;
        assert_eq!(
            message(check_heat_categories(&facts, &[])),
            "Die Vorlaufklasse gibt es nicht."
        );
    }

    #[test]
    fn groups_start_skipped() {
        assert!(heat_starts_skipped(Some(Discipline::new(
            ActType::GroupSmall,
            false
        ))));
        assert!(!heat_starts_skipped(Some(single(false))));
        assert!(!heat_starts_skipped(None));
    }

    #[test]
    fn a_heat_in_finals_or_with_judges_is_not_deletable() {
        assert!(check_heat_deletable(&[], 0).is_ok());
        assert!(message(check_heat_deletable(&["A".into()], 0)).contains("in dem Finale „A“;"));
        assert!(
            message(check_heat_deletable(&["A".into(), "B".into()], 3))
                .contains("in den Finals „A“, „B“;")
        );
        assert!(matches!(
            check_heat_deletable(&[], 1),
            Err(RegistrationError::Conflict(m)) if m.contains("JurorInnen")
        ));
        assert!(check_jury_unit_unused(0).is_ok());
    }

    #[test]
    fn skip_changes_only_until_something_started() {
        assert!(!check_set_heat_skip("H", true, true, true).unwrap());
        assert!(check_set_heat_skip("H", false, true, false).unwrap());
        assert!(message(check_set_heat_skip("H", false, true, true)).contains("Vorlaufklasse „H“"));
    }

    fn slot_heat(name: Option<&str>, d: Option<Discipline>, other: Option<&str>) -> SlotHeatFacts {
        SlotHeatFacts {
            name: name.map(str::to_string),
            discipline: d,
            other_final: other.map(str::to_string),
        }
    }

    #[test]
    fn final_heats_exist_match_and_fill_one_final() {
        let d = single(true);
        assert!(check_final_heats(d, &[]).is_ok());
        assert!(
            check_final_heats(
                d,
                &[
                    slot_heat(Some("A"), None, None),
                    slot_heat(Some("B"), Some(d), None)
                ]
            )
            .is_ok()
        );
        assert_eq!(
            message(check_final_heats(d, &[slot_heat(None, None, None)])),
            "Die Vorlaufklasse gibt es nicht."
        );
        assert!(
            message(check_final_heats(
                d,
                &[slot_heat(Some("A"), Some(single(false)), None)]
            ))
            .contains("„A“ hat einen anderen Typ")
        );
        assert!(
            message(check_final_heats(
                d,
                &[slot_heat(Some("A"), Some(d), Some("F2"))]
            ))
            .contains("im Finale „F2“")
        );
    }

    #[test]
    fn slots_are_replaced_only_when_the_groups_change_and_not_after_start() {
        let (a, b) = (HeatId::new(), HeatId::new());
        let same = [SlotGroupInput {
            heat_id: a,
            count: 2,
        }];
        assert!(!check_slot_groups_change([a, a], &same, true).unwrap());
        let other = [SlotGroupInput {
            heat_id: b,
            count: 2,
        }];
        assert!(check_slot_groups_change([a, a], &other, false).unwrap());
        assert!(message(check_slot_groups_change([a, a], &other, true)).contains("begonnen"));
    }

    #[test]
    fn final_delete_and_unpublish_need_it_not_started() {
        assert!(check_final_deletable(false, 0).is_ok());
        assert!(matches!(
            check_final_deletable(true, 0),
            Err(RegistrationError::Conflict(m)) if m.contains("nicht mehr gelöscht")
        ));
        assert!(matches!(
            check_final_deletable(false, 2),
            Err(RegistrationError::Conflict(_))
        ));
        assert!(check_final_published(true, true).is_ok());
        assert!(check_final_published(false, false).is_ok());
        assert!(message(check_final_published(false, true)).contains("Platzierung bleibt"));
    }

    #[test]
    fn a_final_is_plannable_once() {
        let facts = |name: Option<&str>, planned_elsewhere| FinalPlanFacts {
            name: name.map(str::to_string),
            planned_elsewhere,
        };
        assert!(check_final_plannable(&facts(Some("F"), false)).is_ok());
        assert_eq!(
            message(check_final_plannable(&facts(None, false))),
            "Das Finale gibt es nicht."
        );
        assert_eq!(
            message(check_final_plannable(&facts(Some("F"), true))),
            "Das Finale „F“ ist schon im Zeitplan; jedes Finale läuft in einem Block."
        );
    }

    #[test]
    fn placement_rules() {
        let heat = HeatId::new();
        let other_heat = HeatId::new();
        let slot = |number, act_id, started| PlacementSlot {
            id: FinalSlotId::new(),
            heat_id: heat,
            heat_name: "Vorlauf EM".into(),
            heat_code: "EM".into(),
            number,
            act_id,
            started,
        };
        let (a, b) = (ActId::new(), ActId::new());
        let (s1, s2, s3) = (
            slot(1, Some(a), false),
            slot(2, None, false),
            slot(3, None, true),
        );
        let slots = [s1.clone(), s2.clone(), s3.clone()];
        let heats: HashMap<String, HeatId> =
            [("EM".to_string(), heat), ("EF".to_string(), other_heat)].into();
        let acts: HashMap<ActId, PlacedAct> = [
            (a, PlacedAct::InCategory("EM".into())),
            (b, PlacedAct::InCategory("EM".into())),
        ]
        .into();
        let place = |slot: &PlacementSlot, act_id| SlotPlacement {
            slot_id: slot.id,
            act_id,
        };
        // A change to the same act is no change; swaps and empties work.
        assert_eq!(
            plan_placement(&slots, &[place(&s1, Some(a))], &acts, &heats).unwrap(),
            vec![]
        );
        assert_eq!(
            plan_placement(
                &slots,
                &[place(&s1, None), place(&s2, Some(a))],
                &acts,
                &heats
            )
            .unwrap(),
            vec![(s1.id, None), (s2.id, Some(a))]
        );
        // The same act twice.
        assert_eq!(
            message(plan_placement(
                &slots,
                &[place(&s2, Some(a))],
                &acts,
                &heats
            )),
            "Eine Kür kann nur einen Finalplatz eines Finales bekommen."
        );
        // Unknown slot, started slot.
        let ghost = SlotPlacement {
            slot_id: FinalSlotId::new(),
            act_id: None,
        };
        assert_eq!(
            message(plan_placement(&slots, &[ghost], &acts, &heats)),
            "Diesen Finalplatz gibt es in diesem Finale nicht."
        );
        assert_eq!(
            message(plan_placement(
                &slots,
                &[place(&s3, Some(b))],
                &acts,
                &heats
            )),
            "„3. Finalist (EM)“ hat schon begonnen, die Kür bleibt."
        );
        // Acts that don't exist, are withdrawn, or start elsewhere.
        let unknown = ActId::new();
        assert_eq!(
            message(plan_placement(
                &slots,
                &[place(&s2, Some(unknown))],
                &acts,
                &heats
            )),
            "Die Kür für „2. Finalist (EM)“ gibt es nicht."
        );
        let withdrawn: HashMap<ActId, PlacedAct> = [(b, PlacedAct::Unavailable)].into();
        assert!(
            message(plan_placement(
                &slots,
                &[place(&s2, Some(b))],
                &withdrawn,
                &heats
            ))
            .contains("zurückgezogen oder unvollständig")
        );
        let elsewhere: HashMap<ActId, PlacedAct> = [(b, PlacedAct::InCategory("EF".into()))].into();
        assert!(
            message(plan_placement(
                &slots,
                &[place(&s2, Some(b))],
                &elsewhere,
                &heats
            ))
            .contains("startet in EF, einer Kategorie, die nicht zur Vorlaufklasse „Vorlauf EM“")
        );
    }

    #[test]
    fn a_category_in_a_heat_keeps_its_discipline() {
        assert!(check_category_keeps_discipline(None, single(true)).is_ok());
        assert!(
            check_category_keeps_discipline(Some(("EM", "Vorlauf", single(true))), single(true))
                .is_ok()
        );
        assert_eq!(
            message(check_category_keeps_discipline(
                Some(("EM", "Vorlauf", single(true))),
                single(false)
            )),
            "EM gehört zur Vorlaufklasse „Vorlauf“: Typ und Geschlecht bleiben, solange die Kategorie dort ist."
        );
    }
}
