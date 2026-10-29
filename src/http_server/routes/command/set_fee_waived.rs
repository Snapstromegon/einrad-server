use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    bookings::Actor,
    fee_waivers::{self, Target},
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetFeeWaivedResponse {}

/// Exactly one of `club_id`, `rider_id` and `act_id`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SetFeeWaivedBody {
    /// All the club's fees: start fees and jury fee.
    club_id: Option<Uuid>,
    /// All the rider's starts.
    rider_id: Option<Uuid>,
    /// The act's start, for every rider.
    act_id: Option<Uuid>,
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
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_fee_waived(
    Extension(db): Extension<ReloadableSqlite>,
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
    info!("Setting the fee waiver of {target:?} to {:?}", body.reason);
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;
    fee_waivers::set_waiver(
        &mut tx,
        target,
        body.reason.as_deref(),
        &Actor::of(&granted),
    )
    .await
    .map_err(|e| match e {
        fee_waivers::Error::NotFound => HttpError::NotFound,
        fee_waivers::Error::ReasonMissing => HttpError::ErrorMessages(e.to_string()),
        fee_waivers::Error::Database(e) => HttpError::DBError(e),
    })?;
    tx.commit().await?;
    Ok(Json(SetFeeWaivedResponse {}))
}
