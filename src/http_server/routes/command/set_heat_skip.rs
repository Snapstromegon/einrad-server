use std::sync::Arc;

use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};
use utoipa::ToSchema;

use crate::{
    heats,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    player::PlayerHub,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetHeatSkipResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetHeatSkipBody {
    /// The heat.
    name: String,
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
#[instrument(skip(db, player_hub))]
#[axum::debug_handler]
pub async fn set_heat_skip(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<SetHeatSkipBody>,
) -> Result<Json<SetHeatSkipResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;
    let skip = sqlx::query_scalar!("SELECT skip FROM heats WHERE name = ?", body.name)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;
    if skip == body.skip {
        return Ok(Json(SetHeatSkipResponse {}));
    }
    if heats::has_started(&mut tx, &body.name).await? {
        return Err(HttpError::ErrorMessages(format!(
            "In der Vorlaufklasse „{}“ oder ihrem Finale hat schon eine Kür begonnen; ob ihr Vorlauf entfällt, lässt sich nicht mehr ändern.",
            body.name
        )));
    }
    info!(
        "Setting the Vorlauf of heat {:?} skipped: {}",
        body.name, body.skip
    );
    sqlx::query!(
        "UPDATE heats SET skip = ? WHERE name = ?",
        body.skip,
        body.name
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    // The next act may be another one now.
    if let Err(e) = player_hub.cue_from_timeplan().await {
        warn!("Couldn't cue the next song: {e}");
    }
    Ok(Json(SetHeatSkipResponse {}))
}
