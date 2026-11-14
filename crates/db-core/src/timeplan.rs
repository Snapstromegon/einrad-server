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
        earliest_start_time: Option<&str>,
        after_id: Option<i64>,
    ) -> Result<i64, RegistrationError>;

    /// Overwrites an entry. Acts leaving its block must be able to go back
    /// to a home block that hasn't ended. `NotFound` for an unknown entry.
    async fn edit_timeplan_entry(
        &self,
        id: i64,
        fields: &EntryFields,
        earliest_start_time: Option<&str>,
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
}
