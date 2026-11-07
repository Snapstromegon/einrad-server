use std::sync::Arc;

use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};
use utoipa::ToSchema;

use crate::{
    finals,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    player::PlayerHub,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetFinalPublishedResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetFinalPublishedBody {
    /// The final.
    name: String,
    published: bool,
}

/// Publish (or withdraw) the placement of a final.
///
/// Once published, the timeplan shows the placed acts in the final's slots
/// and the player plays their songs. It can't be withdrawn once the final
/// has started.
#[utoipa::path(
    post,
    tags=["command", "final"],
    path="/set_final_published",
    request_body=SetFinalPublishedBody,
    responses(
        (status=200, content_type="application/json", body=SetFinalPublishedResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, player_hub))]
#[axum::debug_handler]
pub async fn set_final_published(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(player_hub): Extension<Arc<PlayerHub>>,
    _: Granted<permit::ManageResults>,
    Json(body): Json<SetFinalPublishedBody>,
) -> Result<Json<SetFinalPublishedResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;

    if !body.published && finals::has_started(&mut tx, &body.name).await? {
        return Err(HttpError::ErrorMessages(
            "Das Finale hat schon begonnen, seine Platzierung bleibt veröffentlicht.".to_string(),
        ));
    }
    info!(
        "Setting the placement of final {:?} published: {}",
        body.name, body.published
    );
    let result = sqlx::query!(
        "UPDATE finals SET placement_published = ? WHERE name = ?",
        body.published,
        body.name
    )
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }

    tx.commit().await?;
    // The final's songs may be coming up now (or no longer).
    if let Err(e) = player_hub.cue_from_timeplan().await {
        warn!("Couldn't cue the next song: {e}");
    }
    Ok(Json(SetFinalPublishedResponse {}))
}
