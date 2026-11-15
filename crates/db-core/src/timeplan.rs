//! Rules of the live timeplan, shared by the prediction, the forward and
//! backward commands and re-planning (`set_block_order`).
//!
//! The timeplan is a list of entries, ordered by `position`: start blocks
//! (`kind = 'acts'`) and custom entries (breaks, award ceremonies, …). What
//! runs in a start block is decided in one place, [`load`]:
//!
//! 1. The block's stored slots (`timeplan_slots`): acts of any category and
//!    breaks, in their order. Acts without a category at the moment
//!    (withdrawn, incomplete groups) are left out, but keep their place.
//! 2. Then the acts of its home category that sit in no block's slots, in
//!    category order (`acts."order"`). Only one block of a category is its
//!    home block (see [`home_blocks`]). So a block without stored slots runs
//!    its category as before, and acts registered later are never lost.
//! 3. Slots that have started come first (keeping their order), so "Weiter"
//!    and "Zurück" always see the done part of a block as a prefix.
//!
//! A block running a final (`timeplan.final_id`) holds the final's reserved
//! slots instead ("1. Finalist (JE EM)", see `crate::finals`). They run and
//! are judged like acts, with the final's durations. Once the final's
//! placement is published, each slot carries its placed act, whose song
//! plays in it; the act's own progress (in its heat) stays untouched.
//!
//! A heat whose Vorlauf is skipped (`heats.skip`, #344) starts directly in
//! its final, if that is planned: its acts run there as acts (*direct
//! acts*, with their own progress and their category's durations), its
//! finalist slots go unused, and its categories' blocks run none of its
//! acts ("Vorlauf entfällt"), so they take no time unless other acts or
//! breaks sit in them. A final's block runs its stored slots (direct acts,
//! finalist slots by heat and number, breaks), then its other finalist
//! slots, then its other direct acts in category order. Running the Vorlauf
//! again puts everything back where it was.
//!
//! A start block is *empty* when it has no acts (or finalist slots) at the
//! moment of reading. Empty blocks are skipped: they take no time, and
//! "Weiter" never stops on them. A skipped block is marked as started and
//! ended at the same time, so "ended and empty" means "skipped".

use crate::ids::{ActId, CategoryId, FinalId, FinalSlotId, HeatId};
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{DbError, clubs::RegistrationError};
use time::OffsetDateTime;

/// The kind of a timeplan entry, stored as text in `timeplan.kind`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum EntryKind {
    /// A start block: acts and breaks.
    Acts,
    /// A break, an award ceremony, …: a label and a duration.
    Custom,
}

impl EntryKind {
    /// Only custom entries can be internal timeplan entries: a start block's
    /// acts are on the public startlist.
    pub fn can_be_internal(self) -> bool {
        self == Self::Custom
    }

    /// The kind of an entry sent without one: entries with a category (or a
    /// final) are start blocks, as before there were kinds.
    pub fn derive(kind: Option<Self>, has_category_or_final: bool) -> Self {
        kind.unwrap_or(if has_category_or_final {
            Self::Acts
        } else {
            Self::Custom
        })
    }
}

/// The fields of a timeplan entry as stored, checked for its kind.
#[derive(Debug, PartialEq, Eq)]
pub struct EntryFields {
    pub kind: EntryKind,
    /// The home category, by id.
    pub category_id: Option<CategoryId>,
    /// The final the block runs, by id.
    pub final_id: Option<FinalId>,
    pub label: Option<String>,
    pub duration_seconds: Option<i64>,
    pub einfahrzeit_seconds: Option<i64>,
    /// Custom entries: photos next to the clock on the info board.
    pub show_photos: bool,
    /// Custom entries: only the organizer team sees it (Interner
    /// Zeitplaneintrag).
    pub internal: bool,
}

impl EntryFields {
    /// Checks the fields sent for an entry. Start blocks need a category, a
    /// final or a label (e.g. "Nachstarter"); custom entries a label and a
    /// duration. Fields that don't belong to the kind are dropped.
    pub fn new(
        kind: Option<EntryKind>,
        category_id: Option<CategoryId>,
        final_id: Option<FinalId>,
        label: Option<String>,
        duration_seconds: Option<i64>,
        einfahrzeit_seconds: Option<i64>,
    ) -> Result<Self, &'static str> {
        let label = label
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty());
        let kind = EntryKind::derive(kind, category_id.is_some() || final_id.is_some());
        match kind {
            EntryKind::Acts => {
                if category_id.is_some() && final_id.is_some() {
                    return Err("Ein Startblock hat eine Kategorie oder ein Finale, nicht beides.");
                }
                if category_id.is_none() && final_id.is_none() && label.is_none() {
                    return Err(
                        "Ein Startblock braucht eine Kategorie, ein Finale oder eine Bezeichnung.",
                    );
                }
                if einfahrzeit_seconds.is_some_and(|s| s < 0) {
                    return Err("Die Einfahrzeit kann nicht negativ sein.");
                }
                Ok(Self {
                    kind,
                    category_id,
                    final_id,
                    label,
                    duration_seconds: None,
                    einfahrzeit_seconds,
                    show_photos: true,
                    internal: false,
                })
            }
            EntryKind::Custom => {
                if category_id.is_some() || final_id.is_some() {
                    return Err("Eigene Einträge haben keine Kategorie und kein Finale.");
                }
                if label.is_none() || !duration_seconds.is_some_and(|d| d > 0) {
                    return Err("Eigene Einträge brauchen eine Bezeichnung und eine Dauer.");
                }
                Ok(Self {
                    kind,
                    category_id,
                    final_id,
                    label,
                    duration_seconds,
                    einfahrzeit_seconds: None,
                    show_photos: true,
                    internal: false,
                })
            }
        }
    }

    /// Custom entries: with or without photos next to the clock. Start
    /// blocks always keep them.
    #[must_use]
    pub fn with_photos(self, show_photos: bool) -> Self {
        Self {
            show_photos: show_photos || self.kind == EntryKind::Acts,
            ..self
        }
    }

    /// Custom entries: internal or public (see [`EntryKind::can_be_internal`]).
    pub fn with_internal(self, internal: bool) -> Result<Self, &'static str> {
        if internal && !self.kind.can_be_internal() {
            return Err("Nur eigene Einträge können intern sein, keine Startblöcke.");
        }
        Ok(Self { internal, ..self })
    }
}

/// A timeplan entry, as far as the warm-up rules care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    Acts {
        einfahrzeit_seconds: i64,
        empty: bool,
    },
    Custom,
}

/// The warm-up each entry really gets.
///
/// - An empty block gets none.
/// - A block with Einfahrzeit 0 shares the warm-up of the category before
///   it. If that category is empty, its warm-up moves on to the next
///   non-empty category sharing it, so its riders still warm up.
/// - Nobody warms up across a custom entry (a break or an award ceremony).
pub fn effective_warmups(blocks: &[Block]) -> Vec<i64> {
    // The warm-up of an empty block, waiting for the next block sharing it.
    let mut pending = 0;
    blocks
        .iter()
        .map(|block| match *block {
            Block::Custom => {
                pending = 0;
                0
            }
            Block::Acts {
                einfahrzeit_seconds,
                empty: true,
            } => {
                if einfahrzeit_seconds > 0 {
                    pending = einfahrzeit_seconds;
                }
                0
            }
            Block::Acts {
                einfahrzeit_seconds,
                empty: false,
            } => {
                let warmup = if einfahrzeit_seconds > 0 {
                    einfahrzeit_seconds
                } else {
                    pending
                };
                pending = 0;
                warmup
            }
        })
        .collect()
}

/// A category (or a final) as the timeplan needs it.
#[derive(Debug, Clone)]
pub struct CategoryInfo {
    pub description: String,
    pub einfahrzeit_seconds: i64,
    pub act_duration_seconds: i64,
    pub judge_duration_seconds: i64,
}

/// An act in a start block.
#[derive(Debug, Clone)]
pub struct ActSlot {
    pub id: ActId,
    pub name: String,
    /// The act's own category, which may differ from the block's.
    pub category_id: CategoryId,
    /// Its name, for messages.
    pub category: String,
    pub started_at: Option<OffsetDateTime>,
    pub ended_at: Option<OffsetDateTime>,
    pub music_started_at: Option<OffsetDateTime>,
    /// Whether it sits in its block's stored slots, rather than coming from
    /// the block's home category.
    pub stored: bool,
}

/// A break in a start block, e.g. an extra warm-up.
#[derive(Debug, Clone)]
pub struct BreakSlot {
    pub id: i64,
    pub label: String,
    pub duration_seconds: i64,
    pub started_at: Option<OffsetDateTime>,
    pub ended_at: Option<OffsetDateTime>,
}

/// A reserved slot of a final, e.g. "3. Finalist (E EM 1)".
#[derive(Debug, Clone)]
pub struct FinalistSlot {
    pub id: FinalSlotId,
    pub final_id: FinalId,
    pub final_name: String,
    pub heat_id: HeatId,
    /// The heat's short code.
    pub group_label: String,
    /// Counts within the group.
    pub number: i64,
    /// The act placed in the slot, once the placement is published. Its
    /// times are its heat's; the slot's are in the fields here.
    pub act: Option<ActSlot>,
    pub started_at: Option<OffsetDateTime>,
    pub ended_at: Option<OffsetDateTime>,
    /// When the placed act's music first started while the slot ran.
    pub music_started_at: Option<OffsetDateTime>,
}

impl FinalistSlot {
    /// "3. Finalist (E EM 1)".
    pub fn label(&self) -> String {
        crate::finals::finalist_label(self.number, &self.group_label)
    }
}

#[derive(Debug, Clone)]
pub enum Slot {
    Act(ActSlot),
    Break(BreakSlot),
    Finalist(FinalistSlot),
}

impl Slot {
    /// The act of an act slot.
    pub fn act(&self) -> Option<&ActSlot> {
        match self {
            Slot::Act(act) => Some(act),
            Slot::Break(_) | Slot::Finalist(_) => None,
        }
    }

    /// Whether something is performed and judged in it: an act or a
    /// finalist, not a break.
    pub fn is_start(&self) -> bool {
        matches!(self, Slot::Act(_) | Slot::Finalist(_))
    }

    pub fn started_at(&self) -> Option<OffsetDateTime> {
        match self {
            Slot::Act(act) => act.started_at,
            Slot::Break(pause) => pause.started_at,
            Slot::Finalist(finalist) => finalist.started_at,
        }
    }

    pub fn ended_at(&self) -> Option<OffsetDateTime> {
        match self {
            Slot::Act(act) => act.ended_at,
            Slot::Break(pause) => pause.ended_at,
            Slot::Finalist(finalist) => finalist.ended_at,
        }
    }

    pub fn is_started(&self) -> bool {
        self.started_at().is_some()
    }

    pub fn is_running(&self) -> bool {
        self.started_at().is_some() && self.ended_at().is_none()
    }

    pub fn is_ended(&self) -> bool {
        self.ended_at().is_some()
    }
}

/// The label an internal timeplan entry has for the public.
pub const INTERNAL_PUBLIC_LABEL: &str = "Pause";

/// A timeplan entry with its content and live state.
#[derive(Debug)]
pub struct Entry {
    pub id: i64,
    pub kind: EntryKind,
    pub earliest_start_time: Option<OffsetDateTime>,
    /// Custom entries only.
    pub duration_seconds: Option<i64>,
    /// Custom entries: their name. Start blocks: an own name instead of the
    /// home category's, e.g. "Nachstarter".
    pub label: Option<String>,
    /// Start blocks: the home category, if any, by id and name.
    pub category_id: Option<CategoryId>,
    pub category: Option<String>,
    /// Start blocks: the final running here instead of a category, by id
    /// and name.
    pub final_id: Option<FinalId>,
    pub final_name: Option<String>,
    /// Start blocks: overrides the home category's Einfahrzeit.
    pub einfahrzeit_override: Option<i64>,
    /// Custom entries: photos next to the clock on the info board.
    pub show_photos: bool,
    /// Custom entries: only the organizer team sees it.
    pub internal: bool,
    pub started_at: Option<OffsetDateTime>,
    pub ended_at: Option<OffsetDateTime>,
    /// Start blocks: what runs in them, in order (see the module docs).
    pub slots: Vec<Slot>,
    /// Start blocks: whether the order was edited by hand (stored slots).
    pub manually_ordered: bool,
    /// Category blocks of a heat whose Vorlauf is skipped (see the module
    /// docs): they run none of its acts.
    pub preliminary_skipped: bool,
    /// The configured warm-up: the override or the home category's (or
    /// final's).
    pub einfahrzeit_seconds: i64,
    /// The warm-up this entry really gets (see [`effective_warmups`]).
    pub warmup_seconds: i64,
}

impl Entry {
    /// A start block without acts (or finalists); "Weiter" skips it.
    pub fn is_empty(&self) -> bool {
        self.kind == EntryKind::Acts && !self.slots.iter().any(Slot::is_start)
    }

    /// An empty start block that "Weiter" skipped.
    pub fn is_skipped(&self) -> bool {
        self.is_empty() && self.started_at.is_some() && self.ended_at.is_some()
    }

    pub fn is_running(&self) -> bool {
        self.started_at.is_some() && self.ended_at.is_none()
    }
}

/// The timeplan with every block's content.
#[derive(Debug)]
pub struct Timeplan {
    pub entries: Vec<Entry>,
    pub categories: HashMap<CategoryId, CategoryInfo>,
    /// The finals by id, with their durations.
    pub finals: HashMap<FinalId, CategoryInfo>,
    /// The acts that have a category, by id.
    pub acts: HashMap<ActId, ActSlot>,
    /// Which block each act sits in.
    pub block_of_act: HashMap<ActId, i64>,
    /// The acts of skipped heats, with the final they start in directly.
    pub direct_acts: HashMap<ActId, FinalId>,
}

/// The home block of each category, from `(id, category, started)` of the
/// start blocks in order: the first block of the category that has started,
/// otherwise the first one. So moving or inserting entries never takes
/// started acts away from their block.
pub fn home_blocks(
    blocks: impl IntoIterator<Item = (i64, CategoryId, bool)>,
) -> HashMap<CategoryId, i64> {
    let mut homes: HashMap<CategoryId, (i64, bool)> = HashMap::new();
    for (id, category, started) in blocks {
        match homes.get(&category) {
            None => {
                homes.insert(category, (id, started));
            }
            Some(&(_, false)) if started => {
                homes.insert(category, (id, true));
            }
            Some(_) => {}
        }
    }
    homes
        .into_iter()
        .map(|(category, (id, _))| (category, id))
        .collect()
}

impl Timeplan {
    /// The block an act without a stored slot runs in (see [`home_blocks`]),
    /// leaving out the block `without` (about to be deleted or changed).
    pub fn home_block(&self, category_id: CategoryId, without: Option<i64>) -> Option<&Entry> {
        let homes = home_blocks(self.entries.iter().filter_map(|e| {
            (e.kind == EntryKind::Acts && Some(e.id) != without).then_some((
                e.id,
                e.category_id?,
                e.started_at.is_some(),
            ))
        }));
        let id = homes.get(&category_id)?;
        self.entries.iter().find(|e| e.id == *id)
    }

    /// Checks that acts leaving their slots can go back to their home block:
    /// one that has ended would never run them. `without` as in
    /// [`Self::home_block`].
    pub fn check_way_home<'a>(
        &self,
        acts: impl IntoIterator<Item = &'a ActSlot>,
        without: Option<i64>,
    ) -> Result<(), String> {
        let stranded: Vec<String> = acts
            .into_iter()
            .filter(|act| act.started_at.is_none())
            // A direct act's home is its final's block, not its category's.
            .filter(|act| !self.direct_acts.contains_key(&act.id))
            .filter(|act| {
                self.home_block(act.category_id, without)
                    .is_some_and(|home| home.ended_at.is_some())
            })
            .map(|act| {
                let name = if act.name.trim().is_empty() {
                    "ohne Titel"
                } else {
                    act.name.trim()
                };
                format!("„{name}“ ({})", act.category)
            })
            .collect();
        if stranded.is_empty() {
            return Ok(());
        }
        Err(format!(
            "Der Startblock ihrer Kategorie ist schon beendet, dort würden sie nie aufgerufen: {}. Bitte erst in einen anderen Block verschieben.",
            stranded.join(", ")
        ))
    }
}

/// Who a loaded timeplan is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    /// Everything, internal timeplan entries included.
    OrganizerTeam,
    /// Internal timeplan entries become a "Pause" of the same length, so
    /// every other entry keeps its time.
    Public,
}

/// The label an internal timeplan entry has for the public.
/// A timeplan entry as the list shows it, without its content.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct TimeplanListEntry {
    pub id: i64,
    pub kind: EntryKind,
    #[serde(with = "time::serde::iso8601::option")]
    pub earliest_start_time: Option<OffsetDateTime>,
    pub duration_seconds: Option<i64>,
    pub label: Option<String>,
    /// Start blocks only: the home category, by id and name.
    pub category_id: Option<CategoryId>,
    pub category: Option<String>,
    /// Start blocks only: the final running here instead of a category, by
    /// id and name.
    pub final_id: Option<FinalId>,
    pub final_name: Option<String>,
    /// Start blocks only: overrides the home category's (or final's)
    /// Einfahrzeit.
    pub einfahrzeit_seconds: Option<i64>,
    /// Custom entries: photos next to the clock on the info board.
    pub show_photos: bool,
    /// Custom entries: an internal timeplan entry. Only the organizer team
    /// gets it as it is; everyone else gets a "Pause" of the same length.
    pub internal: bool,
    /// Start blocks only: the order was edited by hand (`set_block_order`),
    /// so the category's start order no longer applies to it.
    pub manually_ordered: bool,
    #[serde(with = "time::serde::iso8601::option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::iso8601::option")]
    pub ended_at: Option<OffsetDateTime>,
}

/// One slot of a start block, as sent to `set_block_order`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub enum BlockSlotInput {
    /// An act, from this block or any other (if it hasn't started). In a
    /// final's block: an act of a heat whose Vorlauf is skipped, which
    /// starts directly in this final.
    Act { act_id: ActId },
    /// A final's block only: one of its finalist slots, by heat and number
    /// ("2. Finalist (E EM 1)").
    Finalist { heat_id: HeatId, number: i64 },
    /// A break. With `id`: an existing break of this block, whose label and
    /// duration change, also while it runs. Without: a new break.
    Break {
        id: Option<i64>,
        label: String,
        duration_seconds: i64,
    },
}

fn invalid(message: &str) -> RegistrationError {
    RegistrationError::Rejected(message.to_string())
}

/// Checks a new order of block `timeplan_id` against the loaded timeplan.
/// `heat_codes` has the short codes of the heats named by finalist inputs
/// (for messages). Pure: the backend loads, checks, then writes.
pub fn check_block_order(
    timeplan: &Timeplan,
    timeplan_id: i64,
    slots: &[BlockSlotInput],
    heat_codes: &HashMap<HeatId, String>,
) -> Result<(), RegistrationError> {
    let block = timeplan
        .entries
        .iter()
        .find(|e| e.id == timeplan_id)
        .ok_or(RegistrationError::NotFound)?;
    if block.kind != EntryKind::Acts {
        return Err(invalid("Nur Startblöcke haben eine Reihenfolge."));
    }
    let final_id = block.final_id;
    let final_name = block.final_name.as_deref();
    if block.ended_at.is_some() {
        return Err(invalid("Der Block ist schon beendet."));
    }

    // The started slots come first (see the module docs) and stay.
    let started = block.slots.iter().take_while(|s| s.is_started()).count();
    let kept = slots.len() >= started
        && block.slots[..started]
            .iter()
            .zip(slots)
            .all(|(slot, input)| match (slot, input) {
                (Slot::Act(act), BlockSlotInput::Act { act_id }) => act.id == *act_id,
                (Slot::Break(pause), BlockSlotInput::Break { id, .. }) => Some(pause.id) == *id,
                (Slot::Finalist(finalist), BlockSlotInput::Finalist { heat_id, number }) => {
                    finalist.heat_id == *heat_id && finalist.number == *number
                }
                _ => false,
            });
    if !kept {
        return Err(invalid(
            "Gestartete und beendete Einträge bleiben am Anfang des Blocks, wie sie sind.",
        ));
    }

    let own_breaks = own_breaks(block);
    let mut acts = HashSet::new();
    // The started breaks can't come again later.
    let mut breaks: HashSet<i64> = slots[..started]
        .iter()
        .filter_map(|input| match input {
            BlockSlotInput::Break { id, .. } => *id,
            BlockSlotInput::Act { .. } | BlockSlotInput::Finalist { .. } => None,
        })
        .collect();
    let mut finalists = HashSet::new();
    for input in &slots[started..] {
        match input {
            BlockSlotInput::Act { act_id } => {
                let act = timeplan
                    .acts
                    .get(act_id)
                    .ok_or_else(|| invalid("Unbekannte Kür oder Kür ohne Kategorie."))?;
                if act.started_at.is_some() {
                    return Err(invalid(&format!(
                        "„{}“ hat schon begonnen und bleibt, wo sie ist.",
                        act.name
                    )));
                }
                match (final_id, timeplan.direct_acts.get(act_id)) {
                    (Some(id), Some(direct)) if id == *direct => {}
                    (Some(_), _) => {
                        return Err(invalid(&format!(
                            "„{}“ startet nicht direkt im Finale „{}“: ins Finale kommen nur die Küren von Vorlaufklassen, deren Vorlauf entfällt.",
                            act.name,
                            final_name.unwrap_or_default()
                        )));
                    }
                    (None, Some(direct)) => {
                        let direct = timeplan
                            .finals
                            .get(direct)
                            .map(|f| f.description.as_str())
                            .unwrap_or_default();
                        return Err(invalid(&format!(
                            "„{}“ startet direkt im Finale „{direct}“, ihr Vorlauf entfällt.",
                            act.name
                        )));
                    }
                    (None, None) => {}
                }
                if !acts.insert(*act_id) {
                    return Err(invalid("Eine Kür kann nur einmal im Block stehen."));
                }
            }
            BlockSlotInput::Finalist { heat_id, number } => {
                if final_id.is_none() {
                    return Err(invalid("Finalplätze stehen nur im Block ihres Finales."));
                }
                let heat = block.slots[started..].iter().find_map(|slot| match slot {
                    Slot::Finalist(f) if f.heat_id == *heat_id && f.number == *number => {
                        Some(f.group_label.as_str())
                    }
                    _ => None,
                });
                let open = heat.is_some();
                // For the message: the heat's code, also if the slot isn't
                // open.
                let heat_name = match heat {
                    Some(code) => code.to_string(),
                    None => heat_codes.get(heat_id).cloned().unwrap_or_default(),
                };
                let label = crate::finals::finalist_label(*number, &heat_name);
                if !open {
                    return Err(invalid(&format!(
                        "„{label}“ gibt es in diesem Finale nicht, er hat schon begonnen oder der Vorlauf seiner Vorlaufklasse entfällt."
                    )));
                }
                if !finalists.insert((heat_id, *number)) {
                    return Err(invalid(&format!(
                        "„{label}“ kann nur einmal im Block stehen."
                    )));
                }
            }
            BlockSlotInput::Break { id: Some(id), .. } => {
                if !own_breaks.contains(id) || !breaks.insert(*id) {
                    return Err(invalid("Unbekannte Pause."));
                }
            }
            BlockSlotInput::Break { id: None, .. } => {}
        }
    }
    for input in slots {
        if let BlockSlotInput::Break {
            label,
            duration_seconds,
            ..
        } = input
        {
            if label.trim().is_empty() {
                return Err(invalid("Eine Pause braucht eine Bezeichnung."));
            }
            if *duration_seconds <= 0 {
                return Err(invalid("Eine Pause braucht eine Dauer."));
            }
        }
    }

    // Acts left out go back to their home block, which mustn't be over.
    let left_out = block.slots.iter().filter_map(|slot| match slot {
        Slot::Act(act) if !acts.contains(&act.id) => Some(act),
        _ => None,
    });
    timeplan
        .check_way_home(left_out, None)
        .map_err(RegistrationError::Rejected)
}

/// The ids of a block's own breaks.
pub fn own_breaks(block: &Entry) -> HashSet<i64> {
    block
        .slots
        .iter()
        .filter_map(|slot| match slot {
            Slot::Break(pause) => Some(pause.id),
            Slot::Act(_) | Slot::Finalist(_) => None,
        })
        .collect()
}

/// The live timeplan and its entries (#238). Every method is one unit of
/// work; "Weiter" and "Zurück" read and write in one transaction, so two
/// operators can't both act on the same state.
#[async_trait::async_trait]
pub trait TimeplanRepo: Send + Sync {
    /// The whole timeplan with every block's content, read consistently, as
    /// `audience` may see it.
    async fn load_timeplan(&self, audience: Audience) -> Result<Timeplan, DbError>;

    /// The entries in order, without their content.
    async fn list_timeplan(&self, audience: Audience) -> Result<Vec<TimeplanListEntry>, DbError>;

    /// Adds an entry at the end, or after entry `after_id` (`NotFound` if
    /// there is none). A final must exist and run in no other block.
    async fn add_timeplan_entry(
        &self,
        fields: &EntryFields,
        earliest_start_time: Option<OffsetDateTime>,
        after_id: Option<i64>,
    ) -> Result<i64, RegistrationError>;

    /// Overwrites an entry. Acts leaving its block must be able to go back
    /// to a home block that hasn't ended. `NotFound` for an unknown entry.
    async fn edit_timeplan_entry(
        &self,
        id: i64,
        fields: &EntryFields,
        earliest_start_time: Option<OffsetDateTime>,
    ) -> Result<(), RegistrationError>;

    /// Deletes an entry; its acts go back to their home blocks.
    async fn delete_timeplan_entry(&self, id: i64) -> Result<(), RegistrationError>;

    /// Swaps an entry with its previous (`up`) or next one. `false` if there
    /// is no such neighbour.
    async fn move_timeplan_entry(&self, id: i64, up: bool) -> Result<bool, DbError>;

    /// Sets which acts and breaks run in a start block, in which order.
    async fn set_block_order(
        &self,
        timeplan_id: i64,
        slots: &[BlockSlotInput],
    ) -> Result<(), RegistrationError>;

    /// Puts a start block back to category order.
    async fn reset_block_order(&self, timeplan_id: i64) -> Result<(), RegistrationError>;

    /// One step forward in the live timeplan. Returns whether an act (or
    /// finalist) was ended in it, so the caller can stop the music.
    async fn timeplan_forward(&self) -> Result<bool, DbError>;

    /// Undoes the last forward step.
    async fn timeplan_backward(&self) -> Result<(), DbError>;

    /// Records that the music of a running act started `seconds_ago`, unless
    /// it did before. Returns whether it was recorded.
    async fn mark_music_started(&self, act_id: ActId, seconds_ago: f64) -> Result<bool, DbError>;
}

/// Which way the live timeplan steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// "Weiter".
    Forward,
    /// "Zurück": undoes the last forward step.
    Backward,
}

/// A slot to write to, by what it is stored as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotRef {
    Act(ActId),
    Break(i64),
    Finalist(FinalSlotId),
}

impl Slot {
    /// What the backend needs to find the slot.
    pub fn slot_ref(&self) -> SlotRef {
        match self {
            Slot::Act(act) => SlotRef::Act(act.id),
            Slot::Break(pause) => SlotRef::Break(pause.id),
            Slot::Finalist(finalist) => SlotRef::Finalist(finalist.id),
        }
    }
}

/// One write of a step. Times are not part of it: every `Start`, `End` and
/// `Skip` of a step gets the same "now", which the backend reads once, so
/// "ended in the same step" can be told from the times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOp {
    /// Sets the slot's start time.
    StartSlot(SlotRef),
    /// Sets the slot's end time.
    EndSlot(SlotRef),
    /// Puts a running slot back to planned. An act (or finalist) forgets its
    /// music start.
    UnstartSlot(SlotRef),
    /// Makes an ended slot run again (clears its end time).
    ReopenSlot(SlotRef),
    /// Sets the entry's start time.
    StartEntry(i64),
    /// Sets the entry's end time.
    EndEntry(i64),
    /// Marks an empty entry as started and ended.
    SkipEntry(i64),
    /// Clears the entry's start time.
    UnstartEntry(i64),
    /// Clears the entry's end time.
    ReopenEntry(i64),
    /// Puts a skipped entry back to planned, with its breaks (a block can
    /// lose its acts while one of its breaks runs).
    ResetEntry(i64),
}

/// What one step does: the writes, in order.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Step {
    pub ops: Vec<StepOp>,
    /// Whether an act (or finalist) was ended, so the music stops.
    pub ended_act: bool,
}

/// The writes of one step of the live timeplan over the loaded state. No
/// write if there is nothing to do (nothing to start, nothing to undo).
pub fn next_step(timeplan: &Timeplan, direction: Direction) -> Step {
    match direction {
        Direction::Forward => step_forward(timeplan),
        Direction::Backward => step_backward(timeplan),
    }
}

fn step_forward(timeplan: &Timeplan) -> Step {
    let mut step = Step::default();
    match timeplan.entries.iter().find(|e| e.is_running()) {
        Some(entry) if entry.kind == EntryKind::Acts => {
            let slots = &entry.slots;
            let upcoming = |from: usize| slots[from..].iter().find(|s| !s.is_started());
            let running = slots.iter().position(Slot::is_running);
            if let Some(index) = running {
                step.ops.push(StepOp::EndSlot(slots[index].slot_ref()));
                // A finalist is judged like an act.
                step.ended_act = slots[index].is_start();
            }
            let next = running.map_or_else(|| upcoming(0), |index| upcoming(index + 1));
            match next {
                // That was the last slot, or the block lost its acts (e.g.
                // they were moved away): the block is over. A block without
                // acts doesn't run its breaks either.
                _ if entry.is_empty() => step.ops.push(StepOp::EndEntry(entry.id)),
                None => step.ops.push(StepOp::EndEntry(entry.id)),
                // The act is judged now; the next slot starts with the next
                // step.
                Some(_) if step.ended_act => {}
                // Nothing to judge after a break, or nothing was running:
                // the next slot starts.
                Some(slot) => step.ops.push(StepOp::StartSlot(slot.slot_ref())),
            }
        }
        Some(entry) => step.ops.push(StepOp::EndEntry(entry.id)),
        None => start_next_entry(&timeplan.entries, &mut step.ops),
    }
    step
}

/// Starts the next entry that isn't empty, and skips the empty start blocks
/// before it. Its first slot starts right away if it has no warm-up. If only
/// empty blocks are left, nothing changes.
fn start_next_entry(entries: &[Entry], ops: &mut Vec<StepOp>) {
    let upcoming: Vec<&Entry> = entries.iter().filter(|e| e.started_at.is_none()).collect();
    let Some(next) = upcoming.iter().position(|e| !e.is_empty()) else {
        return;
    };
    ops.extend(upcoming[..next].iter().map(|e| StepOp::SkipEntry(e.id)));
    let next = upcoming[next];
    ops.push(StepOp::StartEntry(next.id));
    if next.kind == EntryKind::Acts
        && next.warmup_seconds == 0
        && let Some(slot) = next.slots.iter().find(|s| !s.is_started())
    {
        ops.push(StepOp::StartSlot(slot.slot_ref()));
    }
}

fn step_backward(timeplan: &Timeplan) -> Step {
    let mut ops = Vec::new();
    let entries = &timeplan.entries;
    match entries.iter().position(Entry::is_running) {
        Some(index) => {
            let entry = &entries[index];
            let slots = &entry.slots;
            if let Some(running) = slots.iter().position(Slot::is_running) {
                ops.push(StepOp::UnstartSlot(slots[running].slot_ref()));
                // Ending a break started this slot in the same step.
                if let Some(pause @ Slot::Break(_)) = running.checked_sub(1).map(|i| &slots[i])
                    && pause.ended_at() == slots[running].started_at()
                {
                    ops.push(StepOp::ReopenSlot(pause.slot_ref()));
                }
            } else if let Some(last) = slots.iter().rfind(|s| s.is_ended()) {
                ops.push(StepOp::ReopenSlot(last.slot_ref()));
            } else {
                // Nothing done in this entry yet: undo its start, together
                // with the empty blocks skipped to get here.
                ops.push(StepOp::UnstartEntry(entry.id));
                let skipped = entries[..index].iter().rev().take_while(|e| e.is_skipped());
                ops.extend(skipped.map(|e| StepOp::ResetEntry(e.id)));
            }
        }
        None => {
            // Reopen the last entry that really ran (and its last slot), and
            // undo the skips after it.
            if let Some(index) = entries
                .iter()
                .rposition(|e| e.ended_at.is_some() && !e.is_skipped())
            {
                let entry = &entries[index];
                ops.push(StepOp::ReopenEntry(entry.id));
                // Its last slot runs again if ending it ended the block; a
                // block that ended because its last acts were moved away
                // just reopens.
                if let Some(last) = entry
                    .slots
                    .iter()
                    .rfind(|s| s.is_ended())
                    .filter(|last| last.ended_at() == entry.ended_at)
                {
                    ops.push(StepOp::ReopenSlot(last.slot_ref()));
                }
                ops.extend(
                    entries[index + 1..]
                        .iter()
                        .filter(|e| e.is_skipped())
                        .map(|e| StepOp::ResetEntry(e.id)),
                );
            }
        }
    }
    Step {
        ops,
        ended_act: false,
    }
}

/// The entry order after moving `id` one place up or down, `None` if it is
/// unknown or has no such neighbour.
pub fn moved_order(ids: &[i64], id: i64, up: bool) -> Option<Vec<i64>> {
    let index = ids.iter().position(|&e| e == id)?;
    let other = if up {
        index.checked_sub(1)
    } else {
        Some(index + 1).filter(|&i| i < ids.len())
    }?;
    let mut ids = ids.to_vec();
    ids.swap(index, other);
    Some(ids)
}

/// The entry order with the new entry `id` right after `after_id`, `None` if
/// there is no such entry. `ids` may already hold `id` (at the end).
pub fn order_after(mut ids: Vec<i64>, id: i64, after_id: i64) -> Option<Vec<i64>> {
    ids.retain(|&e| e != id);
    let index = ids.iter().position(|&e| e == after_id)?;
    ids.insert(index + 1, id);
    Some(ids)
}

/// Whether editing an entry into these fields takes its stored slots away: a
/// final's block holds only the final's slots, a custom entry none.
pub fn drops_slots(fields: &EntryFields) -> bool {
    fields.kind == EntryKind::Custom || fields.final_id.is_some()
}

/// The message for changing the order of a block that already started.
pub fn started_block_keeps_order() -> String {
    "Gestartete Blöcke behalten ihre Reihenfolge.".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn acts(einfahrzeit_seconds: i64, empty: bool) -> Block {
        Block::Acts {
            einfahrzeit_seconds,
            empty,
        }
    }

    #[test]
    fn blocks_keep_their_own_warmup() {
        assert_eq!(
            effective_warmups(&[acts(600, false), acts(300, false)]),
            [600, 300]
        );
    }

    #[test]
    fn empty_blocks_get_no_warmup() {
        assert_eq!(
            effective_warmups(&[acts(600, true), acts(300, false)]),
            [0, 300]
        );
    }

    #[test]
    fn shared_warmup_stays_with_the_first_category() {
        assert_eq!(
            effective_warmups(&[acts(600, false), acts(0, false), acts(0, false)]),
            [600, 0, 0]
        );
    }

    #[test]
    fn shared_warmup_of_an_empty_category_moves_on() {
        assert_eq!(
            effective_warmups(&[
                acts(600, true),
                acts(0, true),
                acts(0, false),
                acts(0, false)
            ]),
            [0, 0, 600, 0]
        );
    }

    #[test]
    fn nobody_warms_up_across_a_break() {
        assert_eq!(
            effective_warmups(&[acts(600, true), Block::Custom, acts(0, false)]),
            [0, 0, 0]
        );
    }

    #[test]
    fn a_new_warmup_replaces_a_pending_one() {
        assert_eq!(
            effective_warmups(&[acts(600, true), acts(300, true), acts(0, false)]),
            [0, 0, 300]
        );
    }

    #[test]
    fn a_started_block_stays_the_home_of_its_category() {
        let (a, b) = (CategoryId::new(), CategoryId::new());
        let homes = home_blocks([(1, a, false), (2, b, false), (3, a, false)]);
        assert_eq!((homes[&a], homes[&b]), (1, 2));
        // Block 3 started before block 1 moved in front of it.
        let homes = home_blocks([(1, a, false), (3, a, true), (4, a, true)]);
        assert_eq!(homes[&a], 3);
    }

    #[test]
    fn entry_fields_are_checked_for_their_kind() {
        let emu11 = CategoryId::new();
        let block = EntryFields::new(None, Some(emu11), None, None, Some(60), Some(300));
        assert_eq!(
            block,
            Ok(EntryFields {
                kind: EntryKind::Acts,
                category_id: Some(emu11),
                final_id: None,
                label: None,
                duration_seconds: None,
                einfahrzeit_seconds: Some(300),
                show_photos: true,
                internal: false,
            })
        );
        // Start blocks always keep their photos, custom entries can drop them.
        assert!(block.unwrap().with_photos(false).show_photos);
        let pause = EntryFields::new(None, None, None, Some("Pause".into()), Some(60), None);
        assert!(!pause.unwrap().with_photos(false).show_photos);
        let late = EntryFields::new(
            Some(EntryKind::Acts),
            None,
            None,
            Some(" Nachstarter ".into()),
            None,
            None,
        );
        assert_eq!(late.unwrap().label.as_deref(), Some("Nachstarter"));
        assert!(
            EntryFields::new(
                Some(EntryKind::Acts),
                None,
                None,
                Some(" ".into()),
                None,
                None
            )
            .is_err()
        );
        assert!(EntryFields::new(None, None, None, Some("Pause".into()), None, None).is_err());
        assert!(EntryFields::new(None, None, None, Some("Pause".into()), Some(0), None).is_err());
        assert!(
            EntryFields::new(
                Some(EntryKind::Custom),
                Some(emu11),
                None,
                Some("Pause".into()),
                Some(60),
                None
            )
            .is_err()
        );
        let pause = EntryFields::new(None, None, None, Some("Pause".into()), Some(60), Some(5));
        assert_eq!(pause.unwrap().einfahrzeit_seconds, None);

        // A final's block, without kind: a start block.
        let id = FinalId::new();
        let final_block = EntryFields::new(None, None, Some(id), None, None, None);
        assert_eq!(final_block.as_ref().unwrap().kind, EntryKind::Acts);
        assert_eq!(final_block.unwrap().final_id, Some(id));
        assert!(EntryFields::new(None, Some(emu11), Some(id), None, None, None).is_err());
        assert!(
            EntryFields::new(
                Some(EntryKind::Custom),
                None,
                Some(id),
                Some("Pause".into()),
                Some(60),
                None
            )
            .is_err()
        );
    }

    #[test]
    fn kinds_are_derived_from_the_category() {
        assert_eq!(EntryKind::derive(None, true), EntryKind::Acts);
        assert_eq!(EntryKind::derive(None, false), EntryKind::Custom);
        assert_eq!(
            EntryKind::derive(Some(EntryKind::Acts), false),
            EntryKind::Acts
        );
    }

    // The live steps.

    fn t(minute: u32) -> Option<OffsetDateTime> {
        Some(
            time::macros::datetime!(2027-05-01 9:00 UTC)
                + time::Duration::minutes(i64::from(minute)),
        )
    }

    fn act(started: Option<u32>, ended: Option<u32>) -> (ActSlot, Slot) {
        let slot = ActSlot {
            id: ActId::new(),
            name: "Act".into(),
            category_id: CategoryId::new(),
            category: "EM".into(),
            started_at: started.and_then(t),
            ended_at: ended.and_then(t),
            music_started_at: None,
            stored: false,
        };
        (slot.clone(), Slot::Act(slot))
    }

    fn pause(started: Option<u32>, ended: Option<u32>) -> Slot {
        Slot::Break(BreakSlot {
            id: 7,
            label: "Pause".into(),
            duration_seconds: 60,
            started_at: started.and_then(t),
            ended_at: ended.and_then(t),
        })
    }

    fn finalist(started: Option<u32>, ended: Option<u32>) -> Slot {
        Slot::Finalist(FinalistSlot {
            id: FinalSlotId::new(),
            final_id: FinalId::new(),
            final_name: "JE EM".into(),
            heat_id: HeatId::new(),
            group_label: "E EM 1".into(),
            number: 1,
            act: None,
            started_at: started.and_then(t),
            ended_at: ended.and_then(t),
            music_started_at: None,
        })
    }

    fn entry(id: i64, kind: EntryKind, slots: Vec<Slot>) -> Entry {
        Entry {
            id,
            kind,
            earliest_start_time: None,
            duration_seconds: None,
            label: None,
            category_id: None,
            category: None,
            final_id: None,
            final_name: None,
            einfahrzeit_override: None,
            show_photos: true,
            internal: false,
            started_at: None,
            ended_at: None,
            slots,
            manually_ordered: false,
            preliminary_skipped: false,
            einfahrzeit_seconds: 0,
            warmup_seconds: 0,
        }
    }

    fn block(id: i64, slots: Vec<Slot>) -> Entry {
        entry(id, EntryKind::Acts, slots)
    }

    fn running(mut e: Entry, start: u32) -> Entry {
        e.started_at = t(start);
        e
    }

    fn done(mut e: Entry, start: u32, end: u32) -> Entry {
        e.started_at = t(start);
        e.ended_at = t(end);
        e
    }

    fn plan(entries: Vec<Entry>) -> Timeplan {
        Timeplan {
            entries,
            categories: HashMap::new(),
            finals: HashMap::new(),
            acts: HashMap::new(),
            block_of_act: HashMap::new(),
            direct_acts: HashMap::new(),
        }
    }

    fn ops(timeplan: &Timeplan, direction: Direction) -> Vec<StepOp> {
        next_step(timeplan, direction).ops
    }

    fn r(slot: &Slot) -> SlotRef {
        slot.slot_ref()
    }

    #[test]
    fn forward_on_an_empty_plan_does_nothing() {
        assert_eq!(
            next_step(&plan(vec![]), Direction::Forward),
            Step::default()
        );
    }

    #[test]
    fn forward_starts_the_first_block_and_its_first_slot() {
        let (_, a) = act(None, None);
        let (_, b) = act(None, None);
        let tp = plan(vec![block(1, vec![a.clone(), b])]);
        let step = next_step(&tp, Direction::Forward);
        assert_eq!(step.ops, [StepOp::StartEntry(1), StepOp::StartSlot(r(&a))]);
        assert!(!step.ended_act);
    }

    #[test]
    fn forward_into_a_block_with_warmup_waits_for_the_next_step() {
        let (_, a) = act(None, None);
        let mut b = block(1, vec![a]);
        b.warmup_seconds = 300;
        assert_eq!(
            ops(&plan(vec![b]), Direction::Forward),
            [StepOp::StartEntry(1)]
        );
    }

    #[test]
    fn forward_skips_empty_blocks_before_the_next_one() {
        let (_, a) = act(None, None);
        let tp = plan(vec![
            done(block(1, vec![]), 0, 0),
            block(2, vec![]),
            block(3, vec![]),
            block(4, vec![a.clone()]),
        ]);
        assert_eq!(
            ops(&tp, Direction::Forward),
            [
                StepOp::SkipEntry(2),
                StepOp::SkipEntry(3),
                StepOp::StartEntry(4),
                StepOp::StartSlot(r(&a))
            ]
        );
    }

    #[test]
    fn forward_with_only_empty_blocks_left_changes_nothing() {
        let tp = plan(vec![block(1, vec![]), entry(2, EntryKind::Custom, vec![])]);
        // The custom entry isn't empty (only start blocks are), so it starts.
        assert_eq!(
            ops(&tp, Direction::Forward),
            [StepOp::SkipEntry(1), StepOp::StartEntry(2)]
        );
        let tp = plan(vec![block(1, vec![]), block(2, vec![])]);
        assert!(ops(&tp, Direction::Forward).is_empty());
    }

    #[test]
    fn forward_starts_a_custom_entry_without_slots() {
        let tp = plan(vec![entry(1, EntryKind::Custom, vec![])]);
        assert_eq!(ops(&tp, Direction::Forward), [StepOp::StartEntry(1)]);
    }

    #[test]
    fn forward_ends_a_running_custom_entry() {
        let tp = plan(vec![
            running(entry(1, EntryKind::Custom, vec![]), 0),
            block(2, vec![act(None, None).1]),
        ]);
        assert_eq!(ops(&tp, Direction::Forward), [StepOp::EndEntry(1)]);
    }

    #[test]
    fn forward_ends_the_running_act_and_waits_for_the_next_step() {
        let (_, a) = act(Some(1), None);
        let (_, b) = act(None, None);
        let tp = plan(vec![running(block(1, vec![a.clone(), b]), 0)]);
        let step = next_step(&tp, Direction::Forward);
        assert_eq!(step.ops, [StepOp::EndSlot(r(&a))]);
        assert!(step.ended_act);
    }

    #[test]
    fn forward_starts_the_next_act_after_it_was_judged() {
        let (_, a) = act(Some(1), Some(2));
        let (_, b) = act(None, None);
        let tp = plan(vec![running(block(1, vec![a, b.clone()]), 0)]);
        let step = next_step(&tp, Direction::Forward);
        assert_eq!(step.ops, [StepOp::StartSlot(r(&b))]);
        assert!(!step.ended_act);
    }

    #[test]
    fn forward_after_a_warmup_starts_the_first_slot() {
        let (_, a) = act(None, None);
        let tp = plan(vec![running(block(1, vec![a.clone()]), 0)]);
        assert_eq!(ops(&tp, Direction::Forward), [StepOp::StartSlot(r(&a))]);
    }

    #[test]
    fn forward_ends_a_running_break_and_starts_the_next_slot_at_once() {
        let (_, b) = act(None, None);
        let p = pause(Some(1), None);
        let tp = plan(vec![running(block(1, vec![p.clone(), b.clone()]), 0)]);
        let step = next_step(&tp, Direction::Forward);
        assert_eq!(step.ops, [StepOp::EndSlot(r(&p)), StepOp::StartSlot(r(&b))]);
        assert!(!step.ended_act);
    }

    #[test]
    fn forward_ends_the_block_after_its_last_act() {
        let (_, a) = act(Some(1), Some(2));
        let (_, b) = act(Some(3), None);
        let tp = plan(vec![running(block(1, vec![a, b.clone()]), 0)]);
        let step = next_step(&tp, Direction::Forward);
        // The act is judged first, the block ends with the next step.
        assert_eq!(step.ops, [StepOp::EndSlot(r(&b)), StepOp::EndEntry(1)]);
        assert!(step.ended_act);
    }

    #[test]
    fn forward_ends_the_block_after_a_trailing_break() {
        let (_, a) = act(Some(1), Some(2));
        let p = pause(Some(2), None);
        let tp = plan(vec![running(block(1, vec![a, p.clone()]), 0)]);
        let step = next_step(&tp, Direction::Forward);
        assert_eq!(step.ops, [StepOp::EndSlot(r(&p)), StepOp::EndEntry(1)]);
        assert!(!step.ended_act);
    }

    #[test]
    fn forward_ends_a_block_that_lost_its_acts_without_running_its_breaks() {
        let p = pause(None, None);
        let tp = plan(vec![running(block(1, vec![p]), 0)]);
        assert_eq!(ops(&tp, Direction::Forward), [StepOp::EndEntry(1)]);
        // Also while one of its breaks runs.
        let p = pause(Some(1), None);
        let tp = plan(vec![running(block(1, vec![p.clone()]), 0)]);
        assert_eq!(
            ops(&tp, Direction::Forward),
            [StepOp::EndSlot(r(&p)), StepOp::EndEntry(1)]
        );
    }

    #[test]
    fn forward_judges_finalists_like_acts() {
        let f = finalist(Some(1), None);
        let g = finalist(None, None);
        let tp = plan(vec![running(block(1, vec![f.clone(), g.clone()]), 0)]);
        let step = next_step(&tp, Direction::Forward);
        assert_eq!(step.ops, [StepOp::EndSlot(r(&f))]);
        assert!(step.ended_act);
        let f = finalist(Some(1), Some(2));
        let tp = plan(vec![running(block(1, vec![f, g.clone()]), 0)]);
        assert_eq!(ops(&tp, Direction::Forward), [StepOp::StartSlot(r(&g))]);
    }

    #[test]
    fn forward_starts_the_block_after_a_finished_one() {
        let (_, a) = act(Some(1), Some(2));
        let (_, b) = act(None, None);
        let tp = plan(vec![
            done(block(1, vec![a]), 0, 3),
            block(2, vec![b.clone()]),
        ]);
        assert_eq!(
            ops(&tp, Direction::Forward),
            [StepOp::StartEntry(2), StepOp::StartSlot(r(&b))]
        );
    }

    #[test]
    fn forward_at_the_end_does_nothing() {
        let tp = plan(vec![done(block(1, vec![act(Some(1), Some(2)).1]), 0, 3)]);
        assert!(ops(&tp, Direction::Forward).is_empty());
    }

    #[test]
    fn backward_with_nothing_started_does_nothing() {
        assert!(ops(&plan(vec![]), Direction::Backward).is_empty());
        let tp = plan(vec![block(1, vec![act(None, None).1])]);
        assert!(ops(&tp, Direction::Backward).is_empty());
    }

    #[test]
    fn backward_unstarts_the_running_act() {
        let (_, a) = act(Some(1), None);
        let tp = plan(vec![running(block(1, vec![a.clone()]), 0)]);
        assert_eq!(ops(&tp, Direction::Backward), [StepOp::UnstartSlot(r(&a))]);
    }

    #[test]
    fn backward_unstarts_an_act_and_reopens_the_break_that_started_it() {
        let (_, a) = act(Some(2), None);
        let p = pause(Some(1), Some(2));
        let tp = plan(vec![running(block(1, vec![p.clone(), a.clone()]), 0)]);
        assert_eq!(
            ops(&tp, Direction::Backward),
            [StepOp::UnstartSlot(r(&a)), StepOp::ReopenSlot(r(&p))]
        );
    }

    #[test]
    fn backward_keeps_a_break_that_ended_earlier() {
        let (_, a) = act(Some(3), None);
        let p = pause(Some(1), Some(2));
        let tp = plan(vec![running(block(1, vec![p, a.clone()]), 0)]);
        assert_eq!(ops(&tp, Direction::Backward), [StepOp::UnstartSlot(r(&a))]);
        // An act in front of the running one isn't a break.
        let (_, first) = act(Some(1), Some(2));
        let (_, second) = act(Some(2), None);
        let tp = plan(vec![running(block(1, vec![first, second.clone()]), 0)]);
        assert_eq!(
            ops(&tp, Direction::Backward),
            [StepOp::UnstartSlot(r(&second))]
        );
    }

    #[test]
    fn backward_reopens_the_last_ended_slot_between_slots() {
        let (_, a) = act(Some(1), Some(2));
        let (_, b) = act(Some(3), Some(4));
        let (_, c) = act(None, None);
        let tp = plan(vec![running(block(1, vec![a, b.clone(), c]), 0)]);
        assert_eq!(ops(&tp, Direction::Backward), [StepOp::ReopenSlot(r(&b))]);
    }

    #[test]
    fn backward_unstarts_a_block_with_nothing_done_and_resets_the_skipped_ones() {
        let tp = plan(vec![
            done(block(1, vec![act(Some(1), Some(2)).1]), 0, 3),
            done(block(2, vec![]), 4, 4),
            done(block(3, vec![]), 4, 4),
            running(block(4, vec![act(None, None).1]), 4),
        ]);
        assert_eq!(
            ops(&tp, Direction::Backward),
            [
                StepOp::UnstartEntry(4),
                StepOp::ResetEntry(3),
                StepOp::ResetEntry(2)
            ]
        );
    }

    #[test]
    fn backward_unstarts_a_running_custom_entry() {
        let tp = plan(vec![
            done(entry(1, EntryKind::Custom, vec![]), 0, 1),
            running(entry(2, EntryKind::Custom, vec![]), 1),
        ]);
        assert_eq!(ops(&tp, Direction::Backward), [StepOp::UnstartEntry(2)]);
    }

    #[test]
    fn backward_reopens_a_block_with_its_last_slot() {
        let (_, a) = act(Some(1), Some(5));
        let tp = plan(vec![done(block(1, vec![a.clone()]), 0, 5)]);
        assert_eq!(
            ops(&tp, Direction::Backward),
            [StepOp::ReopenEntry(1), StepOp::ReopenSlot(r(&a))]
        );
    }

    #[test]
    fn backward_reopens_only_the_block_when_its_last_slot_ended_earlier() {
        // The block ended because its last acts were moved away.
        let (_, a) = act(Some(1), Some(2));
        let tp = plan(vec![done(block(1, vec![a]), 0, 5)]);
        assert_eq!(ops(&tp, Direction::Backward), [StepOp::ReopenEntry(1)]);
        let tp = plan(vec![done(block(1, vec![]), 0, 5)]);
        // An empty block that ended is a skipped one: nothing really ran.
        assert!(ops(&tp, Direction::Backward).is_empty());
    }

    #[test]
    fn backward_from_the_end_resets_the_skipped_blocks_after_the_last_real_one() {
        let (_, a) = act(Some(1), Some(5));
        let tp = plan(vec![
            done(block(1, vec![a.clone()]), 0, 5),
            done(block(2, vec![]), 5, 5),
            done(block(3, vec![]), 5, 5),
        ]);
        assert_eq!(
            ops(&tp, Direction::Backward),
            [
                StepOp::ReopenEntry(1),
                StepOp::ReopenSlot(r(&a)),
                StepOp::ResetEntry(2),
                StepOp::ResetEntry(3)
            ]
        );
    }

    #[test]
    fn backward_reopens_the_last_finalist() {
        let f = finalist(Some(1), Some(5));
        let tp = plan(vec![done(block(1, vec![f.clone()]), 0, 5)]);
        assert_eq!(
            ops(&tp, Direction::Backward),
            [StepOp::ReopenEntry(1), StepOp::ReopenSlot(r(&f))]
        );
    }

    #[test]
    fn entries_move_to_their_neighbour() {
        let ids = [1, 2, 3];
        assert_eq!(moved_order(&ids, 2, true), Some(vec![2, 1, 3]));
        assert_eq!(moved_order(&ids, 2, false), Some(vec![1, 3, 2]));
        assert_eq!(moved_order(&ids, 1, true), None);
        assert_eq!(moved_order(&ids, 3, false), None);
        assert_eq!(moved_order(&ids, 9, true), None);
        assert_eq!(moved_order(&[5], 5, false), None);
    }

    #[test]
    fn a_new_entry_goes_right_after_another() {
        assert_eq!(order_after(vec![1, 2, 3, 4], 4, 1), Some(vec![1, 4, 2, 3]));
        assert_eq!(order_after(vec![1, 2, 3, 4], 4, 3), Some(vec![1, 2, 3, 4]));
        assert_eq!(order_after(vec![1, 2, 4], 4, 9), None);
        // After itself isn't a place.
        assert_eq!(order_after(vec![1, 4], 4, 4), None);
    }

    #[test]
    fn custom_and_final_entries_drop_their_slots() {
        let category = CategoryId::new();
        let block = EntryFields::new(None, Some(category), None, None, None, None).unwrap();
        assert!(!drops_slots(&block));
        let custom = EntryFields::new(None, None, None, Some("Pause".into()), Some(60), None);
        assert!(drops_slots(&custom.unwrap()));
        let final_block = EntryFields::new(None, None, Some(FinalId::new()), None, None, None);
        assert!(drops_slots(&final_block.unwrap()));
    }
}
