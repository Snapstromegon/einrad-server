//! Heats and finals: the [`HeatsFinalsRepo`] trait.
//!
//! The commands check their rules and write in one unit of work. A refusal
//! rolls everything back: `Rejected` (400), `Conflict` (409), `NotFound` and
//! `NotFoundMessage` (404) carry German messages where there are any.

use crate::{
    DbError,
    clubs::RegistrationError,
    finals::{Final, FinalChange, NewFinal, SlotPlacement},
    heats::{Heat, HeatChange, NewHeat},
    ids::{FinalId, HeatId},
    structure::StructureRows,
};

#[async_trait::async_trait]
pub trait HeatsFinalsRepo {
    /// The heats in the order of their first category; those without
    /// categories last. One consistent read.
    async fn list_heats(&self) -> Result<Vec<Heat>, DbError>;

    /// Adds a heat and assigns its categories (HEAT-1). A heat of groups
    /// starts with its Vorlauf skipped.
    async fn add_heat(&self, heat: &NewHeat) -> Result<(), RegistrationError>;

    /// Edits a heat, its categories included.
    async fn edit_heat(&self, change: &HeatChange) -> Result<(), RegistrationError>;

    /// Deletes a heat; refused while a final's slots or a jury unit use it.
    async fn delete_heat(&self, id: HeatId) -> Result<(), RegistrationError>;

    /// Skips the Vorlauf of a heat or runs it again; `false` if it already
    /// was so. Refused once an act of the heat or its final has started.
    async fn set_heat_skip(&self, id: HeatId, skip: bool) -> Result<bool, RegistrationError>;

    /// The finals in order with their slots. Placed acts are only shown for
    /// published finals unless `with_unpublished`. One consistent read.
    async fn list_finals(&self, with_unpublished: bool) -> Result<Vec<Final>, DbError>;

    /// Adds a final with its slots.
    async fn add_final(&self, new: &NewFinal) -> Result<(), RegistrationError>;

    /// Edits a final; its slots are only created anew when the slot groups
    /// change.
    async fn edit_final(&self, change: &FinalChange) -> Result<(), RegistrationError>;

    /// Deletes a final with its slots and timeplan block; refused once it
    /// has started or while a jury unit uses it.
    async fn delete_final(&self, id: FinalId) -> Result<(), RegistrationError>;

    /// Places acts in the slots of a final (`None` empties a slot); the
    /// slots not named keep theirs. The placement is checked as a whole.
    async fn set_final_placement(
        &self,
        id: FinalId,
        changes: &[SlotPlacement],
    ) -> Result<(), RegistrationError>;

    /// Publishes the placement of a final, or withdraws it (not possible
    /// once the final has started).
    async fn set_final_published(
        &self,
        id: FinalId,
        published: bool,
    ) -> Result<(), RegistrationError>;

    /// Everything the structure tree is built from, in one read.
    async fn structure_rows(&self) -> Result<StructureRows, DbError>;
}
