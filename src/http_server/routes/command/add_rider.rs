use crate::ids::{ClubId, RiderId};
use axum::{Extension, Json, http::StatusCode};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{gender::Gender, people};
use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    system_status::Capabilities,
};
use db_core::riders::NewRider;

#[derive(Debug, Serialize, ToSchema)]
pub struct AddRiderResponse {
    rider_id: RiderId,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddRiderBody {
    club_id: ClubId,
    firstname: String,
    lastname: String,
    #[serde(with = "time::serde::iso8601")]
    birthdate: time::OffsetDateTime,
    /// Required: decides whether the rider's single is Einzel weiblich or
    /// männlich.
    gender: Option<Gender>,
}

/// Add a rider to a club, without starts; those are set with the start
/// commands (`set_single`, `request_pair`, `link_pair`, `leave_pair`).
///
/// Only the club (or an admin) can add riders to it.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/add_rider",
    request_body=AddRiderBody,
    responses(
        (status=200, content_type="application/json", body=AddRiderResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn add_rider(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(mut body): Json<AddRiderBody>,
) -> Result<Json<AddRiderResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let gender = body
        .gender
        .ok_or_else(|| HttpError::ErrorMessages("Bitte gib das Geschlecht an.".to_string()))?;
    people::trim_name(&mut body.firstname);
    people::trim_name(&mut body.lastname);
    let rider_id = db
        .add_rider(
            &auth.actor(),
            &NewRider {
                club_id: body.club_id,
                firstname: body.firstname,
                lastname: body.lastname,
                birthdate: body.birthdate,
                gender,
            },
        )
        .await?;
    Ok(Json(AddRiderResponse { rider_id }))
}
