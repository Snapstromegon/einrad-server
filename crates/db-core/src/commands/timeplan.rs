//! Timeplan commands: the timeplan and the live schedule.
//!
//! The request types are DB-facing: the handler converts its API body into
//! them.

use time::OffsetDateTime;

use crate::{
    actor::Actor,
    error::CommandError,
    ids::{ActId, HeatId},
    timeplan::EntryFields,
};

/// Adds an entry at the end, or after entry `after_id`.
#[derive(Debug, Clone)]
pub struct AddTimeplanEntry {
    /// The checked fields (see [`EntryFields::new`]).
    pub fields: EntryFields,
    pub earliest_start_time: Option<OffsetDateTime>,
    pub after_id: Option<i64>,
}

/// Overwrites an entry.
#[derive(Debug, Clone)]
pub struct EditTimeplanEntry {
    pub id: i64,
    pub fields: EntryFields,
    pub earliest_start_time: Option<OffsetDateTime>,
}

/// Deletes an entry; its acts go back to their home blocks.
#[derive(Debug, Clone)]
pub struct DeleteTimeplanEntry {
    pub id: i64,
}

/// Swaps an entry with its previous one.
#[derive(Debug, Clone)]
pub struct MoveTimeplanUp {
    pub id: i64,
}

/// Swaps an entry with its next one.
#[derive(Debug, Clone)]
pub struct MoveTimeplanDown {
    pub id: i64,
}

/// One slot of a start block.
#[derive(Debug, Clone)]
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

/// Sets which acts and breaks run in a start block, in which order.
#[derive(Debug, Clone)]
pub struct SetBlockOrder {
    pub timeplan_id: i64,
    pub slots: Vec<BlockSlotInput>,
}

/// Puts a start block back to category order.
#[derive(Debug, Clone)]
pub struct ResetBlockOrder {
    pub timeplan_id: i64,
}

/// Records that the music of a running act just started, unless it did
/// before.
#[derive(Debug, Clone)]
pub struct SetActMusicStarted {
    pub act_id: ActId,
}

/// Publishes (`at`) or withdraws the start list and timeplan.
#[derive(Debug, Clone)]
pub struct SetStartlistPublished {
    pub published: bool,
    /// The time to store as the publication time.
    pub at: OffsetDateTime,
}

/// The write side of the timeplan area of the API; one method per endpoint.
/// "Weiter" and "Zurück" read and write in one transaction, so two operators
/// can't both act on the same state.
#[async_trait::async_trait]
pub trait TimeplanCommands: Send + Sync {
    /// Gives the new entry's id. A final must exist and run in no other
    /// block; `NotFound` if `after_id` names no entry.
    async fn add_timeplan_entry(
        &self,
        actor: &Actor,
        cmd: AddTimeplanEntry,
    ) -> Result<i64, CommandError>;

    /// Acts leaving its block must be able to go back to a home block that
    /// hasn't ended. `NotFound` for an unknown entry.
    async fn edit_timeplan_entry(
        &self,
        actor: &Actor,
        cmd: EditTimeplanEntry,
    ) -> Result<(), CommandError>;

    /// `NotFound` for an unknown entry; refused when an act couldn't go
    /// back to its home block.
    async fn delete_timeplan_entry(
        &self,
        actor: &Actor,
        cmd: DeleteTimeplanEntry,
    ) -> Result<(), CommandError>;

    /// `false` (nothing changed) if there is no previous entry.
    async fn move_timeplan_up(
        &self,
        actor: &Actor,
        cmd: MoveTimeplanUp,
    ) -> Result<bool, CommandError>;

    /// `false` (nothing changed) if there is no next entry.
    async fn move_timeplan_down(
        &self,
        actor: &Actor,
        cmd: MoveTimeplanDown,
    ) -> Result<bool, CommandError>;

    /// The order is checked by `timeplan::check_block_order`.
    async fn set_block_order(&self, actor: &Actor, cmd: SetBlockOrder) -> Result<(), CommandError>;

    /// Blocks that have started keep their order (`Rejected`).
    async fn reset_block_order(
        &self,
        actor: &Actor,
        cmd: ResetBlockOrder,
    ) -> Result<(), CommandError>;

    /// One step forward in the live timeplan. Returns whether an act (or
    /// finalist) was ended in it, so the caller can stop the music.
    async fn timeplan_forward(&self, actor: &Actor) -> Result<bool, CommandError>;

    /// Undoes the last forward step.
    async fn timeplan_backward(&self, actor: &Actor) -> Result<(), CommandError>;

    /// Returns whether the start was recorded (only the first start of a
    /// running act counts).
    async fn set_act_music_started(
        &self,
        actor: &Actor,
        cmd: SetActMusicStarted,
    ) -> Result<bool, CommandError>;

    /// `false` without a competition.
    async fn set_startlist_published(
        &self,
        actor: &Actor,
        cmd: SetStartlistPublished,
    ) -> Result<bool, CommandError>;
}
