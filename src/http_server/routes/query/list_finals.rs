use crate::ids::{ActId, FinalId, FinalSlotId, HeatId};
use axum::{Extension, Json};
use db_core::{prelude::*, queries::finals as read};
use serde::Serialize;
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    act_type::ActType,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

/// A run of slots as the admin API returns it.
#[derive(Debug, Serialize, ToSchema)]
pub struct SlotGroupInfo {
    pub heat_id: HeatId,
    /// The heat's readable name.
    pub heat: String,
    /// The heat's short code.
    pub heat_code: String,
    pub count: i64,
}

/// A reserved slot of a final.
#[derive(Debug, Serialize, ToSchema)]
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
#[derive(Debug, Serialize, ToSchema)]
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

impl From<read::SlotGroupInfo> for SlotGroupInfo {
    fn from(group: read::SlotGroupInfo) -> Self {
        Self {
            heat_id: group.heat_id,
            heat: group.heat,
            heat_code: group.heat_code,
            count: group.count,
        }
    }
}

impl From<read::FinalSlot> for FinalSlot {
    fn from(slot: read::FinalSlot) -> Self {
        Self {
            id: slot.id,
            label: slot.label,
            heat_id: slot.heat_id,
            group_label: slot.group_label,
            number: slot.number,
            act_id: slot.act_id,
            started: slot.started,
        }
    }
}

impl From<read::Final> for Final {
    fn from(f: read::Final) -> Self {
        Self {
            id: f.id,
            name: f.name,
            code: f.code,
            description: f.description,
            act_type: f.act_type,
            is_single_male: f.is_single_male,
            order: f.order,
            einfahrzeit_seconds: f.einfahrzeit_seconds,
            act_duration_seconds: f.act_duration_seconds,
            judge_duration_seconds: f.judge_duration_seconds,
            slot_groups: f.slot_groups.into_iter().map(SlotGroupInfo::from).collect(),
            categories: f.categories,
            slots: f.slots.into_iter().map(FinalSlot::from).collect(),
            placement_published: f.placement_published,
            started: f.started,
            timeplan_id: f.timeplan_id,
        }
    }
}

/// List the finals, in order.
#[utoipa::path(
    get,
    tags=["query", "final"],
    path="/list_finals",
    responses(
        (status=200, content_type="application/json", body=Vec<Final>),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn list_finals(
    Extension(repo): Extension<DbHandle>,
    results: Option<Granted<permit::ManageResults>>,
) -> Result<Json<Vec<Final>>, HttpError> {
    // Who manages results also sees acts placed in unpublished finals.
    let actor = results.map(|granted| granted.actor());
    Ok(Json(
        repo.list_finals(actor.as_ref())
            .await?
            .into_iter()
            .map(Final::from)
            .collect(),
    ))
}
