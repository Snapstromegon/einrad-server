use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
    timeplan::swap_with_neighbour,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct MoveTimeplanUpResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MoveTimeplanUpBody {
    id: i64,
}

/// Move a timeplan entry up by swapping places with the previous entry.
///
/// This endpoint is used to reorder timeplan entries.
#[utoipa::path(
    post,
    tags=["command", "timeplan"],
    path="/move_timeplan_up",
    request_body=MoveTimeplanUpBody,
    responses(
        (status=200, content_type="application/json", body=MoveTimeplanUpResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn move_timeplan_up(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<MoveTimeplanUpBody>,
) -> Result<Json<MoveTimeplanUpResponse>, HttpError> {
    let db = db.get().await.clone();

    let mut tx = begin_write(&db).await?;
    if !swap_with_neighbour(&mut tx, body.id, true).await? {
        // No previous entry to swap with.
        return Err(HttpError::StatusCode(StatusCode::BAD_REQUEST));
    }
    tx.commit().await?;

    Ok(Json(MoveTimeplanUpResponse {}))
}
