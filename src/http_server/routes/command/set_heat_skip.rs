use crate::ids::HeatId;
use std::sync::Arc;

use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{instrument, warn};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    player::PlayerHub,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetHeatSkipResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetHeatSkipBody {
    /// The heat.
    id: HeatId,
    /// Skip its Vorlauf (its acts start directly in its final) or run it.
    skip: bool,
}

/// Skip a heat's Vorlauf or run it again (Konzept §1.5). The tool suggests
/// skipping when the heat has fewer acts than its threshold; the organizer
/// decides here.
///
/// While skipped, its categories' blocks stay in the timeplan but run none
/// of its acts; they start in the final that its slots belong to, if that is
/// planned. Once an act of the heat or its final has started, it can't
/// change.
#[utoipa::path(
    post,
    tags=["command", "heat"],
    path="/set_heat_skip",
    request_body=SetHeatSkipBody,
    responses(
        (status=200, content_type="application/json", body=SetHeatSkipResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo, player_hub))]
#[axum::debug_handler]
pub async fn set_heat_skip(
    Extension(repo): Extension<DbHandle>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<SetHeatSkipBody>,
) -> Result<Json<SetHeatSkipResponse>, HttpError> {
    if repo.set_heat_skip(body.id, body.skip).await? {
        // The next act may be another one now.
        if let Err(e) = player_hub.cue_from_timeplan().await {
            warn!("Couldn't cue the next song: {e}");
        }
    }
    Ok(Json(SetHeatSkipResponse {}))
}
