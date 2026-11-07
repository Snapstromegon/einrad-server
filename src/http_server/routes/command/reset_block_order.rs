use std::sync::Arc;

use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    player::PlayerHub,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
    timeplan::{Slot, load},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct ResetBlockOrderResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ResetBlockOrderBody {
    timeplan_id: i64,
}

/// Puts a start block back to category order.
///
/// Deletes the block's stored order and its breaks. Acts that came from
/// other blocks go back to their home blocks, unless one of those has ended;
/// acts of its category that sit in other blocks stay there. Blocks that have
/// started keep their order.
#[utoipa::path(
    post,
    tags=["command", "timeplan"],
    path="/reset_block_order",
    request_body=ResetBlockOrderBody,
    responses(
        (status=200, content_type="application/json", body=ResetBlockOrderResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, player_hub))]
#[axum::debug_handler]
pub async fn reset_block_order(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<ResetBlockOrderBody>,
) -> Result<Json<ResetBlockOrderResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;
    let timeplan = load(&mut tx).await?;
    let block = timeplan
        .entries
        .iter()
        .find(|e| e.id == body.timeplan_id)
        .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;
    if block.started_at.is_some() {
        return Err(HttpError::ErrorMessages(
            "Gestartete Blöcke behalten ihre Reihenfolge.".to_string(),
        ));
    }
    if block.final_name.is_some() {
        return Err(HttpError::ErrorMessages(
            "Ein Finale hat feste Finalplätze; die ändern sich im Finale selbst.".to_string(),
        ));
    }
    // Acts from other blocks go back to theirs, which mustn't be over.
    timeplan
        .check_way_home(block.slots.iter().filter_map(Slot::act), None)
        .map_err(HttpError::ErrorMessages)?;

    info!("Resetting the order of timeplan block {}", body.timeplan_id);
    sqlx::query!(
        "DELETE FROM timeplan_slots WHERE timeplan_id = ?",
        body.timeplan_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    if let Err(e) = player_hub.cue_from_timeplan().await {
        warn!("Couldn't cue the next song: {e}");
    }
    Ok(Json(ResetBlockOrderResponse {}))
}
