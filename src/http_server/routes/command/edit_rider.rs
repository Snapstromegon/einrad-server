use crate::ids::RiderId;
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
use db_core::riders::RiderEdit;

#[derive(Debug, Serialize, ToSchema)]
pub struct EditRiderResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditRiderBody {
    rider_id: RiderId,
    firstname: String,
    lastname: String,
    #[serde(with = "time::serde::iso8601")]
    birthdate: time::OffsetDateTime,
    /// Missing keeps the stored gender. A single follows it to Einzel
    /// weiblich or männlich.
    #[serde(default)]
    gender: Option<Gender>,
}

/// Edit a rider's own data; their starts are changed with the start
/// commands.
///
/// Only the rider's club (or an admin) can edit them.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/edit_rider",
    request_body=EditRiderBody,
    responses(
        (status=200, content_type="application/json", body=EditRiderResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn edit_rider(
    Extension(db): Extension<DbHandle>,
    auth: Auth,
    capabilities: Capabilities,
    Json(mut body): Json<EditRiderBody>,
) -> Result<Json<EditRiderResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    people::trim_name(&mut body.firstname);
    people::trim_name(&mut body.lastname);
    db.edit_rider(
        &auth.actor(),
        body.rider_id,
        &RiderEdit {
            firstname: body.firstname,
            lastname: body.lastname,
            birthdate: body.birthdate,
            gender: body.gender,
        },
    )
    .await?;
    Ok(Json(EditRiderResponse {}))
}
