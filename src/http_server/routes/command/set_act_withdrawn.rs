use crate::ids::ActId;
use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetActWithdrawnResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetActWithdrawnBody {
    act_id: ActId,
    withdrawn: bool,
}

/// Withdraws an act after the registration deadline, or takes it back.
///
/// A withdrawn act doesn't start: it leaves the startlist and the
/// timeplan and shows no category. Its riders still pay for it unless the fee
/// is waived (`set_fee_waived`; the announcement refunds only with a medical
/// certificate); withdrawing doesn't change a waiver.
/// Before the deadline, clubs simply delete their riders instead.
#[utoipa::path(
    post,
    tags=["command", "admin"],
    path="/set_act_withdrawn",
    request_body=SetActWithdrawnBody,
    responses(
        (status=200, content_type="application/json", body=SetActWithdrawnResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_act_withdrawn(
    Extension(db): Extension<DbHandle>,
    _: Granted<permit::ManageRegistrations>,
    Json(body): Json<SetActWithdrawnBody>,
) -> Result<Json<SetActWithdrawnResponse>, HttpError> {
    info!("Setting act {} withdrawn = {}", body.act_id, body.withdrawn);
    if !db.set_act_withdrawn(body.act_id, body.withdrawn).await? {
        return Err(HttpError::NotFound);
    }

    Ok(Json(SetActWithdrawnResponse {}))
}
