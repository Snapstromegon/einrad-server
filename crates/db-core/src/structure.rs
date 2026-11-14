//! The raw rows the structure tree is built from.

use crate::{
    acts::ActParticipant,
    heats::Discipline,
    ids::{ActId, CategoryId, FinalId, HeatId},
};

#[derive(Debug, Clone)]
pub struct StructureCategory {
    pub id: CategoryId,
    pub name: String,
    pub description: Option<String>,
    pub discipline: Discipline,
    pub heat_id: Option<HeatId>,
}

#[derive(Debug, Clone)]
pub struct StructureHeat {
    pub id: HeatId,
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub skip: bool,
}

#[derive(Debug, Clone)]
pub struct StructureFinal {
    pub id: FinalId,
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub discipline: Discipline,
}

/// The slots of one heat in a final.
#[derive(Debug, Clone, Copy)]
pub struct StructureSlots {
    pub final_id: FinalId,
    pub heat_id: HeatId,
    pub slots: i64,
}

/// An act with the category it ranks in, if any.
#[derive(Debug, Clone)]
pub struct StructureAct {
    pub id: ActId,
    /// The start order within the category; `None` while not placed.
    pub order: Option<i64>,
    pub name: String,
    pub group_name: Option<String>,
    pub category_id: Option<CategoryId>,
    pub discipline: Discipline,
    pub participants: Vec<ActParticipant>,
    pub withdrawn: bool,
}

/// One consistent read of everything the structure tree shows.
#[derive(Debug, Clone)]
pub struct StructureRows {
    /// In category order.
    pub categories: Vec<StructureCategory>,
    /// By name.
    pub heats: Vec<StructureHeat>,
    /// In final order.
    pub finals: Vec<StructureFinal>,
    /// Each final's slot groups in slot order.
    pub slot_groups: Vec<StructureSlots>,
    pub acts: Vec<StructureAct>,
}
