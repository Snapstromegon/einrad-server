//! Heat queries: heats, and the structure tree's rows.
//!
//! The read models are DB-facing and carry no serialization; the handler maps
//! them to its response types.

use crate::{
    act_type::ActType,
    actor::Actor,
    acts::ActParticipant,
    error::QueryError,
    heats::Discipline,
    ids::{ActId, CategoryId, FinalId, HeatId},
};

/// A final a heat fills slots of, as the heat list shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct HeatFinal {
    pub id: FinalId,
    /// The readable name.
    pub name: String,
    /// The short code.
    pub code: String,
}

/// A heat as the API lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Heat {
    pub id: HeatId,
    /// The readable name, unique, e.g. "Einzel männlich – Expert 1 (U17–U21)".
    pub name: String,
    /// The short code, unique, e.g. "E EM 1".
    pub code: String,
    pub description: Option<String>,
    /// Its categories, in category order. Empty until the admin assigns
    /// them.
    pub categories: Vec<String>,
    /// The same categories by id, in the same order.
    pub category_ids: Vec<CategoryId>,
    /// The act type its categories share; `None` without categories.
    pub act_type: Option<ActType>,
    /// Whether its categories are singles male; `None` without categories.
    pub is_single_male: Option<bool>,
    /// The finals whose slots it fills; at most one.
    pub finals: Vec<HeatFinal>,
    /// The fewest acts it needs for its Vorlauf.
    pub threshold: i64,
    /// Whether its Vorlauf is skipped: its acts start directly in its final.
    pub skip: bool,
    /// Its acts at the moment, without withdrawn ones.
    pub act_count: i64,
    /// Whether the tool suggests skipping its Vorlauf: it has fewer acts than
    /// its threshold; never for groups, which start in their final. The
    /// organizer decides.
    pub skip_suggested: bool,
}

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

/// The read side of the heat area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait HeatQueries: Send + Sync {
    /// The heats in the order of their first category; those without
    /// categories last. One consistent read. Open to everyone.
    async fn list_heats(&self) -> Result<Vec<Heat>, QueryError>;

    /// Everything the structure tree is built from, in one read.
    async fn get_structure(&self, actor: &Actor) -> Result<StructureRows, QueryError>;
}
