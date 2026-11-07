use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
    timeplan::{Slot, load},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteTimeplanEntryResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteTimeplanEntryBody {
    id: i64, // Primary key of the timeplan entry to delete
}

/// Delete a timeplan entry.
///
/// This endpoint is used to delete an existing timeplan entry. The acts of a
/// start block go back to their home blocks; it is refused if one of those
/// has ended already.
#[utoipa::path(
    post,
    tags=["command", "timeplan"],
    path="/delete_timeplan_entry",
    request_body=DeleteTimeplanEntryBody,
    responses(
        (status=200, content_type="application/json", body=DeleteTimeplanEntryResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn delete_timeplan_entry(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<DeleteTimeplanEntryBody>,
) -> Result<Json<DeleteTimeplanEntryResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;

    // Its acts go back to their home blocks, which mustn't be over.
    let timeplan = load(&mut tx).await?;
    if let Some(block) = timeplan.entries.iter().find(|e| e.id == body.id) {
        timeplan
            .check_way_home(block.slots.iter().filter_map(Slot::act), Some(block.id))
            .map_err(HttpError::ErrorMessages)?;
    }

    let result = sqlx::query!(
        r#"
        DELETE FROM timeplan
        WHERE id = $1
        "#,
        body.id,
    )
    .execute(&mut *tx)
    .await?;

    if result.rows_affected() == 0 {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }
    tx.commit().await?;

    Ok(Json(DeleteTimeplanEntryResponse {}))
}
