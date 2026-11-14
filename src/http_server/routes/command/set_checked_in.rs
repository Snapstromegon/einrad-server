use crate::ids::RiderId;
use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetCheckedInResponse {
    /// The check-in time set for riders that weren't checked in yet. Riders
    /// that already were keep their earlier time. `null` when checking out.
    #[serde(with = "time::serde::iso8601::option")]
    checked_in_at: Option<OffsetDateTime>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetCheckedInBody {
    rider_ids: Vec<RiderId>,
    checked_in: bool,
}

/// Check riders in (or out) on competition day.
///
/// Check-in and music control. Works after registration closed. Fails as a
/// whole if one of the riders doesn't exist.
#[utoipa::path(
    post,
    tags=["command", "check-in"],
    path="/set_checked_in",
    request_body=SetCheckedInBody,
    responses(
        (status=200, content_type="application/json", body=SetCheckedInResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn set_checked_in(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::CheckIn>,
    Json(body): Json<SetCheckedInBody>,
) -> Result<Json<SetCheckedInResponse>, HttpError> {
    let checked_in_at = body.checked_in.then(OffsetDateTime::now_utc);
    info!(
        "Setting check-in of {} riders to {:?}",
        body.rider_ids.len(),
        checked_in_at
    );
    repo.set_checked_in(&body.rider_ids, checked_in_at).await?;
    Ok(Json(SetCheckedInResponse { checked_in_at }))
}
