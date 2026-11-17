use crate::ids::{ActId, ClubId, RiderId};
use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    fee_waivers::Target,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetFeeWaivedResponse {}

/// Exactly one of `club_id`, `rider_id` and `act_id`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SetFeeWaivedBody {
    /// All the club's fees: start fees and jury fee.
    club_id: Option<ClubId>,
    /// All the rider's starts.
    rider_id: Option<RiderId>,
    /// The act's start, for every rider.
    act_id: Option<ActId>,
    /// Why the fees are waived, e.g. "Veranstalter"; `null` charges them
    /// again.
    reason: Option<String>,
}

/// Waive the fees of a club, a rider or an act in full, or charge them
/// again. Treasurer.
#[utoipa::path(
    post,
    tags=["command", "payment"],
    path="/set_fee_waived",
    request_body=SetFeeWaivedBody,
    responses(
        (status=200, content_type="application/json", body=SetFeeWaivedResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn set_fee_waived(
    Extension(repo): Extension<DbHandle>,
    granted: Granted<permit::ManagePayments>,
    Json(body): Json<SetFeeWaivedBody>,
) -> Result<Json<SetFeeWaivedResponse>, HttpError> {
    let target = match (body.club_id, body.rider_id, body.act_id) {
        (Some(id), None, None) => Target::Club(id),
        (None, Some(id), None) => Target::Rider(id),
        (None, None, Some(id)) => Target::Act(id),
        _ => {
            return Err(HttpError::ErrorMessages(
                "Bitte genau einen Verein, eine FahrerIn oder eine Kür angeben.".to_string(),
            ));
        }
    };
    repo.set_fee_waiver(&granted.booking_actor(), target, body.reason.as_deref())
        .await?;
    Ok(Json(SetFeeWaivedResponse {}))
}
