//! Final queries: finals.
//!
//! The read models are DB-facing and carry no serialization; the handler maps
//! them to its response types.

use crate::{
    act_type::ActType,
    actor::Actor,
    error::QueryError,
    ids::{ActId, FinalId, FinalSlotId, HeatId},
    roles::Permission,
};

/// A run of slots as the admin API returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotGroupInfo {
    pub heat_id: HeatId,
    /// The heat's readable name.
    pub heat: String,
    /// The heat's short code.
    pub heat_code: String,
    pub count: i64,
}

/// A reserved slot of a final.
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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

/// Whether `actor` sees the acts placed in a final before the placement is
/// published: who manages results.
#[must_use]
pub fn sees_unpublished_placement(actor: Option<&Actor>) -> bool {
    actor.is_some_and(|actor| actor.can(Permission::ManageResults))
}

/// The read side of the final area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait FinalQueries: Send + Sync {
    /// The finals in order with their slots. Placed acts are only shown for
    /// published finals unless the actor
    /// [sees unpublished placements](sees_unpublished_placement). One
    /// consistent read.
    async fn list_finals(&self, actor: Option<&Actor>) -> Result<Vec<Final>, QueryError>;
}
