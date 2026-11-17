use crate::ids::{ActId, FinalId, FinalSlotId};
use std::sync::Arc;

use axum::{Extension, Json};
use db_core::{DbHandle, finals::SlotPlacement};
use serde::{Deserialize, Serialize};
use tracing::{instrument, warn};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    player::PlayerHub,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetFinalPlacementResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct FinalSlotPlacement {
    slot_id: FinalSlotId,
    /// The act placed in the slot; `null` empties it.
    act_id: Option<ActId>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetFinalPlacementBody {
    /// The final.
    id: FinalId,
    /// The slots to change; the others keep their acts.
    slots: Vec<FinalSlotPlacement>,
}

/// Place acts in the slots of a final.
///
/// A slot takes the acts of its heat's categories, each at most once per
/// final.
/// A slot that has started keeps its act. Once the placement is published,
/// changes show in the timeplan right away.
#[utoipa::path(
    post,
    tags=["command", "final"],
    path="/set_final_placement",
    request_body=SetFinalPlacementBody,
    responses(
        (status=200, content_type="application/json", body=SetFinalPlacementResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo, player_hub))]
#[axum::debug_handler]
pub async fn set_final_placement(
    Extension(repo): Extension<DbHandle>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    _: Granted<permit::ManageResults>,
    Json(body): Json<SetFinalPlacementBody>,
) -> Result<Json<SetFinalPlacementResponse>, HttpError> {
    let changes: Vec<SlotPlacement> = body
        .slots
        .iter()
        .map(|slot| SlotPlacement {
            slot_id: slot.slot_id,
            act_id: slot.act_id,
        })
        .collect();
    repo.set_final_placement(body.id, &changes).await?;
    // A published final's songs coming up may have changed.
    if let Err(e) = player_hub.cue_from_timeplan().await {
        warn!("Couldn't cue the next song: {e}");
    }
    Ok(Json(SetFinalPlacementResponse {}))
}
