use axum::{Extension, Json, http::StatusCode};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
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
#[instrument(skip(repo))]
pub async fn set_startlist_published(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<SetStartlistPublishedBody>,
) -> Result<Json<SetStartlistPublishedResponse>, HttpError> {
    info!("Start list published: {}", body.published);
    if !repo
        .set_startlist_published(body.published, time::OffsetDateTime::now_utc())
        .await?
    {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }
    Ok(Json(SetStartlistPublishedResponse {}))
}
