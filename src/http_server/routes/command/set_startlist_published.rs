use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetStartlistPublishedResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetStartlistPublishedBody {
    published: bool,
}

/// Approve the start list and the timeplan of the current competition for
/// the public, or take the approval back. Until then only the organizer
/// team sees them; afterwards changes show right away. Admin only.
#[utoipa::path(
    post,
    tags=["command", "timeplan"],
    path="/set_startlist_published",
    request_body=SetStartlistPublishedBody,
    responses(
        (status=200, content_type="application/json", body=SetStartlistPublishedResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn set_startlist_published(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<SetStartlistPublishedBody>,
) -> Result<Json<SetStartlistPublishedResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;
    let published_at = body.published.then(time::OffsetDateTime::now_utc);
    info!("Start list published: {}", body.published);
    let result = sqlx::query!(
        "UPDATE competition SET startlist_published_at = ?",
        published_at
    )
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }
    tx.commit().await?;
    Ok(Json(SetStartlistPublishedResponse {}))
}
