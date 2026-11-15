use crate::ids::RiderId;
use axum::{Extension, Json};
use db_core::{prelude::*, queries::check_in::CheckIn as CheckInRow};
use time::OffsetDateTime;
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

/// The check-in state of a rider.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct CheckIn {
    rider_id: RiderId,
    /// `null` if the rider hasn't checked in yet.
    #[serde(with = "time::serde::iso8601::option")]
    checked_in_at: Option<OffsetDateTime>,
}

impl From<CheckInRow> for CheckIn {
    fn from(check_in: CheckInRow) -> Self {
        Self {
            rider_id: check_in.rider_id,
            checked_in_at: check_in.checked_in_at,
        }
    }
}

/// The check-in state of every rider. Check-in and music control.
///
/// Small enough to poll during the competition.
#[utoipa::path(
    get,
    tags=["query", "check-in"],
    path="/list_check_ins",
    responses(
        (status=200, content_type="application/json", body=Vec<CheckIn>),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn list_check_ins(
    Extension(repo): Extension<DbHandle>,
    granted: Granted<permit::CheckIn>,
) -> Result<Json<Vec<CheckIn>>, HttpError> {
    let check_ins = repo.list_check_ins(&granted.actor()).await?;
    Ok(Json(check_ins.into_iter().map(Into::into).collect()))
}
