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
//! A block running a final (`timeplan.final_name`) holds the final's reserved
//! slots instead ("1. Finalist (U11–U15)", see `crate::finals`). They run and
//! are judged like acts, with the final's durations. Once the final's
//! placement is published, each slot carries its placed act, whose song
//! plays in it; the act's own progress (in its heat) stays untouched.
//!
//! A start block is *empty* when it has no acts (or finalist slots) at the
//! moment of reading. Empty blocks are skipped: they take no time, and
//! "Weiter" never stops on them. A skipped block is marked as started and
//! ended at the same time, so "ended and empty" means "skipped".

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use time::OffsetDateTime;
use uuid::Uuid;

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
    pub fn derive(kind: Option<Self>, category: Option<&str>) -> Self {
        kind.unwrap_or(if category.is_some() {
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
    pub category: Option<String>,
    pub final_name: Option<String>,
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
        category: Option<String>,
        final_name: Option<String>,
        label: Option<String>,
        duration_seconds: Option<i64>,
        einfahrzeit_seconds: Option<i64>,
    ) -> Result<Self, &'static str> {
        let category = category.filter(|c| !c.is_empty());
        let final_name = final_name.filter(|f| !f.is_empty());
        let label = label
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty());
        let kind = EntryKind::derive(kind, category.as_deref().or(final_name.as_deref()));
        match kind {
            EntryKind::Acts => {
                if category.is_some() && final_name.is_some() {
                    return Err("Ein Startblock hat eine Kategorie oder ein Finale, nicht beides.");
                }
                if category.is_none() && final_name.is_none() && label.is_none() {
                    return Err(
                        "Ein Startblock braucht eine Kategorie, ein Finale oder eine Bezeichnung.",
                    );
                }
                if einfahrzeit_seconds.is_some_and(|s| s < 0) {
                    return Err("Die Einfahrzeit kann nicht negativ sein.");
                }
                Ok(Self {
                    kind,
                    category,
                    final_name,
                    label,
                    duration_seconds: None,
                    einfahrzeit_seconds,
                    show_photos: true,
                    internal: false,
                })
            }
            EntryKind::Custom => {
                if category.is_some() || final_name.is_some() {
                    return Err("Eigene Einträge haben keine Kategorie und kein Finale.");
                }
                if label.is_none() || !duration_seconds.is_some_and(|d| d > 0) {
                    return Err("Eigene Einträge brauchen eine Bezeichnung und eine Dauer.");
                }
                Ok(Self {
                    kind,
                    category,
                    final_name,
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
    pub id: Uuid,
    pub name: String,
    /// The act's own category, which may differ from the block's.
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

/// A reserved slot of a final, e.g. "3. Finalist (U17–U21)".
#[derive(Debug, Clone)]
pub struct FinalistSlot {
    pub id: Uuid,
    pub final_name: String,
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
    /// "3. Finalist (U17–U21)".
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
    /// Start blocks: the home category, if any.
    pub category: Option<String>,
    /// Start blocks: the final running here instead of a category.
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
    pub categories: HashMap<String, CategoryInfo>,
    /// The finals by name, with their durations.
    pub finals: HashMap<String, CategoryInfo>,
    /// The acts that have a category, by id.
    pub acts: HashMap<Uuid, ActSlot>,
    /// Which block each act sits in.
    pub block_of_act: HashMap<Uuid, i64>,
}

/// The home block of each category, from `(id, category, started)` of the
/// start blocks in order: the first block of the category that has started,
/// otherwise the first one. So moving or inserting entries never takes
/// started acts away from their block.
pub fn home_blocks<'a>(
    blocks: impl IntoIterator<Item = (i64, &'a str, bool)>,
) -> HashMap<&'a str, i64> {
    let mut homes: HashMap<&str, (i64, bool)> = HashMap::new();
    for (id, category, started) in blocks {
        match homes.get(category) {
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
    pub fn home_block(&self, category: &str, without: Option<i64>) -> Option<&Entry> {
        let homes = home_blocks(self.entries.iter().filter_map(|e| {
            (e.kind == EntryKind::Acts && Some(e.id) != without).then_some((
                e.id,
                e.category.as_deref()?,
                e.started_at.is_some(),
            ))
        }));
        let id = homes.get(category)?;
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
            .filter(|act| {
                self.home_block(&act.category, without)
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
pub const INTERNAL_PUBLIC_LABEL: &str = "Pause";

/// Loads the whole timeplan for the organizer team (see [`load_for`]).
pub async fn load(db: &mut SqliteConnection) -> sqlx::Result<Timeplan> {
    load_for(db, Audience::OrganizerTeam).await
}

/// Loads the timeplan in order, with every block's slots, emptiness and
/// effective warm-ups, as `audience` may see it.
pub async fn load_for(db: &mut SqliteConnection, audience: Audience) -> sqlx::Result<Timeplan> {
    let public = audience == Audience::Public;
    let categories: HashMap<String, CategoryInfo> = sqlx::query!(
        r#"
        SELECT
            name as "name!",
            description,
            einfahrzeit_seconds,
            act_duration_seconds,
            judge_duration_seconds
        FROM categories
        "#
    )
    .fetch_all(&mut *db)
    .await?
    .into_iter()
    .map(|row| {
        (
            row.name.clone(),
            CategoryInfo {
                description: row.description.unwrap_or_else(|| row.name.clone()),
                einfahrzeit_seconds: row.einfahrzeit_seconds,
                act_duration_seconds: row.act_duration_seconds,
                judge_duration_seconds: row.judge_duration_seconds,
            },
        )
    })
    .collect();

    let finals: HashMap<String, CategoryInfo> = sqlx::query!(
        r#"
        SELECT
            name as "name!",
            description,
            einfahrzeit_seconds,
            act_duration_seconds,
            judge_duration_seconds
        FROM finals
        "#
    )
    .fetch_all(&mut *db)
    .await?
    .into_iter()
    .map(|row| {
        (
            row.name.clone(),
            CategoryInfo {
                description: row
                    .description
                    .filter(|d| !d.trim().is_empty())
                    .unwrap_or_else(|| row.name.clone()),
                einfahrzeit_seconds: row.einfahrzeit_seconds,
                act_duration_seconds: row.act_duration_seconds,
                judge_duration_seconds: row.judge_duration_seconds,
            },
        )
    })
    .collect();
    let acts_in_order: Vec<ActSlot> = sqlx::query!(
        r#"
        SELECT
            id as "id!: Uuid",
            name,
            category as "category!",
            started_at as "started_at: OffsetDateTime",
            ended_at as "ended_at: OffsetDateTime",
            music_started_at as "music_started_at: OffsetDateTime"
        FROM view_act
        WHERE category IS NOT NULL
        ORDER BY "order"
        "#
    )
    .fetch_all(&mut *db)
    .await?
    .into_iter()
    .map(|row| ActSlot {
        id: row.id,
        name: row.name,
        category: row.category,
        started_at: row.started_at,
        ended_at: row.ended_at,
        music_started_at: row.music_started_at,
        stored: false,
    })
    .collect();
    let acts: HashMap<Uuid, ActSlot> = acts_in_order
        .iter()
        .map(|act| (act.id, act.clone()))
        .collect();

    let mut finalists: HashMap<String, Vec<Slot>> = HashMap::new();
    for row in sqlx::query!(
        r#"
        SELECT
            final_slots.id as "id!: Uuid",
            final_name,
            group_label,
            number,
            act_id as "act_id: Uuid",
            finals.placement_published,
            started_at as "started_at: OffsetDateTime",
            ended_at as "ended_at: OffsetDateTime",
            music_started_at as "music_started_at: OffsetDateTime"
        FROM final_slots
        JOIN finals ON finals.name = final_slots.final_name
        ORDER BY final_name, position
        "#
    )
    .fetch_all(&mut *db)
    .await?
    {
        // A withdrawn act has no category at the moment: the slot shows as
        // open.
        let act = row
            .act_id
            .filter(|_| row.placement_published)
            .and_then(|id| acts.get(&id))
            .cloned();
        finalists
            .entry(row.final_name.clone())
            .or_default()
            .push(Slot::Finalist(FinalistSlot {
                id: row.id,
                final_name: row.final_name,
                group_label: row.group_label,
                number: row.number,
                act,
                started_at: row.started_at,
                ended_at: row.ended_at,
                music_started_at: row.music_started_at,
            }));
    }

    let rows = sqlx::query!(
        r#"
        SELECT
            id as "id!",
            kind as "kind: EntryKind",
            earliest_start_time as "earliest_start_time: OffsetDateTime",
            duration_seconds,
            CASE WHEN internal AND ?1 THEN ?2 ELSE label END as "label: String",
            category,
            final_name,
            einfahrzeit_seconds,
            show_photos,
            internal AND NOT ?1 as "internal!: bool",
            started_at as "started_at: OffsetDateTime",
            ended_at as "ended_at: OffsetDateTime"
        FROM timeplan
        ORDER BY position, id
        "#,
        public,
        INTERNAL_PUBLIC_LABEL,
    )
    .fetch_all(&mut *db)
    .await?;

    let mut stored: HashMap<i64, Vec<Slot>> = HashMap::new();
    // Acts in a block's slots, also those hidden at the moment: they never
    // show up in their home block on top.
    let mut placed: HashSet<Uuid> = HashSet::new();
    let act_blocks: HashSet<i64> = rows
        .iter()
        .filter(|row| row.kind == EntryKind::Acts)
        .map(|row| row.id)
        .collect();
    for row in sqlx::query!(
        r#"
        SELECT
            id as "id!",
            timeplan_id,
            act_id as "act_id: Uuid",
            label,
            duration_seconds,
            started_at as "started_at: OffsetDateTime",
            ended_at as "ended_at: OffsetDateTime"
        FROM timeplan_slots
        ORDER BY timeplan_id, position, id
        "#
    )
    .fetch_all(&mut *db)
    .await?
    {
        if !act_blocks.contains(&row.timeplan_id) {
            continue;
        }
        let slots = stored.entry(row.timeplan_id).or_default();
        match row.act_id {
            Some(act_id) => {
                placed.insert(act_id);
                if let Some(act) = acts.get(&act_id) {
                    slots.push(Slot::Act(ActSlot {
                        stored: true,
                        ..act.clone()
                    }));
                }
            }
            None => slots.push(Slot::Break(BreakSlot {
                id: row.id,
                label: row.label.unwrap_or_default(),
                duration_seconds: row.duration_seconds.unwrap_or(0),
                started_at: row.started_at,
                ended_at: row.ended_at,
            })),
        }
    }

    let homes = home_blocks(rows.iter().filter_map(|row| {
        (row.kind == EntryKind::Acts).then_some((
            row.id,
            row.category.as_deref()?,
            row.started_at.is_some(),
        ))
    }));
    let mut entries: Vec<Entry> = Vec::with_capacity(rows.len());
    for row in &rows {
        let manually_ordered = stored.contains_key(&row.id);
        let mut slots = stored.remove(&row.id).unwrap_or_default();
        let mut einfahrzeit_seconds = 0;
        if row.kind == EntryKind::Acts {
            // A final's block holds its reserved slots (`set_block_order`
            // refuses final blocks, so there are no stored slots).
            if let Some(final_name) = row.final_name.as_deref() {
                slots = finalists.remove(final_name).unwrap_or_default();
                einfahrzeit_seconds = finals.get(final_name).map_or(0, |f| f.einfahrzeit_seconds);
            }
            if let Some(home) = row.category.as_deref() {
                if homes.get(home) == Some(&row.id) {
                    slots.extend(
                        acts_in_order
                            .iter()
                            .filter(|act| act.category == home && !placed.contains(&act.id))
                            .cloned()
                            .map(Slot::Act),
                    );
                }
                einfahrzeit_seconds = categories.get(home).map_or(0, |c| c.einfahrzeit_seconds);
            }
            einfahrzeit_seconds = row.einfahrzeit_seconds.unwrap_or(einfahrzeit_seconds);
            // Stable: started slots first, then the others, each in their
            // planned order. Not by start time: that would trust the clock
            // (and imported data) more than the plan.
            slots.sort_by_key(|slot| !slot.is_started());
        }
        entries.push(Entry {
            id: row.id,
            kind: row.kind,
            earliest_start_time: row.earliest_start_time,
            duration_seconds: row.duration_seconds,
            label: row.label.clone(),
            category: row.category.clone(),
            final_name: row.final_name.clone(),
            einfahrzeit_override: row.einfahrzeit_seconds,
            show_photos: row.show_photos,
            internal: row.internal,
            started_at: row.started_at,
            ended_at: row.ended_at,
            slots,
            manually_ordered,
            einfahrzeit_seconds,
            warmup_seconds: 0,
        });
    }

    let blocks: Vec<Block> = entries
        .iter()
        .map(|entry| match entry.kind {
            // A block without a category (e.g. "Nachstarter") warms up only
            // if it has an own Einfahrzeit; a shared warm-up waiting for the
            // next category passes it by.
            EntryKind::Acts
                if entry.category.is_none()
                    && entry.final_name.is_none()
                    && entry.einfahrzeit_override.is_none() =>
            {
                Block::Acts {
                    einfahrzeit_seconds: 0,
                    empty: true,
                }
            }
            EntryKind::Acts => Block::Acts {
                einfahrzeit_seconds: entry.einfahrzeit_seconds,
                empty: entry.is_empty(),
            },
            EntryKind::Custom => Block::Custom,
        })
        .collect();
    for (entry, warmup) in entries.iter_mut().zip(effective_warmups(&blocks)) {
        entry.warmup_seconds = warmup;
    }

    let block_of_act = entries
        .iter()
        .flat_map(|entry| {
            entry
                .slots
                .iter()
                .filter_map(move |slot| slot.act().map(|act| (act.id, entry.id)))
        })
        .collect();

    Ok(Timeplan {
        entries,
        categories,
        finals,
        acts,
        block_of_act,
    })
}

/// The ids of the timeplan entries in order.
pub async fn entry_ids(db: &mut SqliteConnection) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar!(r#"SELECT id as "id!" FROM timeplan ORDER BY position, id"#)
        .fetch_all(db)
        .await
}

/// Numbers the entries `1..` in the given order.
pub async fn set_entry_order(db: &mut SqliteConnection, ids: &[i64]) -> sqlx::Result<()> {
    for (position, id) in (1_i64..).zip(ids) {
        sqlx::query!(
            "UPDATE timeplan SET position = ? WHERE id = ?",
            position,
            id
        )
        .execute(&mut *db)
        .await?;
    }
    Ok(())
}

/// Swaps an entry with the one before it (`up`) or after it. Returns
/// whether there was one.
pub async fn swap_with_neighbour(
    db: &mut SqliteConnection,
    id: i64,
    up: bool,
) -> sqlx::Result<bool> {
    let mut ids = entry_ids(&mut *db).await?;
    let Some(index) = ids.iter().position(|&e| e == id) else {
        return Ok(false);
    };
    let other = if up {
        index.checked_sub(1)
    } else {
        Some(index + 1).filter(|&i| i < ids.len())
    };
    let Some(other) = other else {
        return Ok(false);
    };
    ids.swap(index, other);
    set_entry_order(db, &ids).await?;
    Ok(true)
}

/// The database's current time, like `datetime('now')` but with
/// milliseconds. One step of the live timeplan writes the same time
/// everywhere, so "ended in the same step" can be told from the times; two
/// steps (separate write transactions) don't share a millisecond.
pub async fn now(db: &mut SqliteConnection) -> sqlx::Result<String> {
    sqlx::query_scalar!(r#"SELECT strftime('%Y-%m-%d %H:%M:%f', 'now') as "now!: String""#)
        .fetch_one(db)
        .await
}

/// Starts a slot at `now` (see [`now`]).
pub async fn start_slot(db: &mut SqliteConnection, slot: &Slot, now: &str) -> sqlx::Result<()> {
    match slot {
        Slot::Act(act) => {
            sqlx::query!("UPDATE acts SET started_at = ? WHERE id = ?", now, act.id)
                .execute(db)
                .await?;
        }
        Slot::Break(pause) => {
            sqlx::query!(
                "UPDATE timeplan_slots SET started_at = ? WHERE id = ?",
                now,
                pause.id
            )
            .execute(db)
            .await?;
        }
        Slot::Finalist(finalist) => {
            sqlx::query!(
                "UPDATE final_slots SET started_at = ? WHERE id = ?",
                now,
                finalist.id
            )
            .execute(db)
            .await?;
        }
    }
    Ok(())
}

/// Ends a running slot at `now` (see [`now`]).
pub async fn end_slot(db: &mut SqliteConnection, slot: &Slot, now: &str) -> sqlx::Result<()> {
    match slot {
        Slot::Act(act) => {
            sqlx::query!("UPDATE acts SET ended_at = ? WHERE id = ?", now, act.id)
                .execute(db)
                .await?;
        }
        Slot::Break(pause) => {
            sqlx::query!(
                "UPDATE timeplan_slots SET ended_at = ? WHERE id = ?",
                now,
                pause.id
            )
            .execute(db)
            .await?;
        }
        Slot::Finalist(finalist) => {
            sqlx::query!(
                "UPDATE final_slots SET ended_at = ? WHERE id = ?",
                now,
                finalist.id
            )
            .execute(db)
            .await?;
        }
    }
    Ok(())
}

/// Puts a running slot back to planned. An act (or finalist) forgets its
/// music start.
pub async fn unstart_slot(db: &mut SqliteConnection, slot: &Slot) -> sqlx::Result<()> {
    match slot {
        Slot::Act(act) => {
            sqlx::query!(
                "UPDATE acts SET started_at = NULL, music_started_at = NULL WHERE id = ?",
                act.id
            )
            .execute(db)
            .await?;
        }
        Slot::Break(pause) => {
            sqlx::query!(
                "UPDATE timeplan_slots SET started_at = NULL WHERE id = ?",
                pause.id
            )
            .execute(db)
            .await?;
        }
        Slot::Finalist(finalist) => {
            sqlx::query!(
                "UPDATE final_slots SET started_at = NULL, music_started_at = NULL WHERE id = ?",
                finalist.id
            )
            .execute(db)
            .await?;
        }
    }
    Ok(())
}

/// Makes an ended slot run again.
pub async fn reopen_slot(db: &mut SqliteConnection, slot: &Slot) -> sqlx::Result<()> {
    match slot {
        Slot::Act(act) => {
            sqlx::query!("UPDATE acts SET ended_at = NULL WHERE id = ?", act.id)
                .execute(db)
                .await?;
        }
        Slot::Break(pause) => {
            sqlx::query!(
                "UPDATE timeplan_slots SET ended_at = NULL WHERE id = ?",
                pause.id
            )
            .execute(db)
            .await?;
        }
        Slot::Finalist(finalist) => {
            sqlx::query!(
                "UPDATE final_slots SET ended_at = NULL WHERE id = ?",
                finalist.id
            )
            .execute(db)
            .await?;
        }
    }
    Ok(())
}

/// Records that the music of a running act started `seconds_ago`, unless it
/// did before: the act timer counts from the first start, not from pressing
/// "Weiter". The act may also run in a final's slot, placed there and
/// published. Returns whether it was recorded.
pub async fn mark_music_started(
    db: &sqlx::SqlitePool,
    act_id: Uuid,
    seconds_ago: f64,
) -> sqlx::Result<bool> {
    let mut tx = crate::reloadable_sqlite::begin_write(db).await?;
    let act = sqlx::query!(
        r#"
        UPDATE acts SET music_started_at = datetime(julianday('now') - ? / 86400.0)
        WHERE id = ? AND started_at IS NOT NULL AND ended_at IS NULL AND music_started_at IS NULL
        "#,
        seconds_ago,
        act_id
    )
    .execute(&mut *tx)
    .await?;
    let finalist = sqlx::query!(
        r#"
        UPDATE final_slots SET music_started_at = datetime(julianday('now') - ? / 86400.0)
        WHERE act_id = ? AND started_at IS NOT NULL AND ended_at IS NULL AND music_started_at IS NULL
            AND final_name IN (SELECT name FROM finals WHERE placement_published)
        "#,
        seconds_ago,
        act_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(act.rows_affected() + finalist.rows_affected() > 0)
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
        let homes = home_blocks([(1, "A", false), (2, "B", false), (3, "A", false)]);
        assert_eq!((homes["A"], homes["B"]), (1, 2));
        // Block 3 started before block 1 moved in front of it.
        let homes = home_blocks([(1, "A", false), (3, "A", true), (4, "A", true)]);
        assert_eq!(homes["A"], 3);
    }

    #[test]
    fn entry_fields_are_checked_for_their_kind() {
        let block = EntryFields::new(None, Some("EMU11".into()), None, None, Some(60), Some(300));
        assert_eq!(
            block,
            Ok(EntryFields {
                kind: EntryKind::Acts,
                category: Some("EMU11".into()),
                final_name: None,
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
                Some("EMU11".into()),
                None,
                Some("Pause".into()),
                Some(60),
                None
            )
            .is_err()
        );
        let pause = EntryFields::new(
            None,
            Some(String::new()),
            Some(String::new()),
            Some("Pause".into()),
            Some(60),
            Some(5),
        );
        assert_eq!(pause.unwrap().einfahrzeit_seconds, None);

        // A final's block, without kind: a start block.
        let final_block = EntryFields::new(None, None, Some("Finale 1".into()), None, None, None);
        assert_eq!(final_block.as_ref().unwrap().kind, EntryKind::Acts);
        assert_eq!(final_block.unwrap().final_name.as_deref(), Some("Finale 1"));
        assert!(
            EntryFields::new(
                None,
                Some("EMU11".into()),
                Some("Finale 1".into()),
                None,
                None,
                None
            )
            .is_err()
        );
        assert!(
            EntryFields::new(
                Some(EntryKind::Custom),
                None,
                Some("Finale 1".into()),
                Some("Pause".into()),
                Some(60),
                None
            )
            .is_err()
        );
    }

    #[test]
    fn kinds_are_derived_from_the_category() {
        assert_eq!(EntryKind::derive(None, Some("EMU11")), EntryKind::Acts);
        assert_eq!(EntryKind::derive(None, None), EntryKind::Custom);
        assert_eq!(
            EntryKind::derive(Some(EntryKind::Acts), None),
            EntryKind::Acts
        );
    }
}
