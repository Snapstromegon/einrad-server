use crate::ids::RiderId;
use axum::{Extension, Json};
use db_core::{prelude::*, queries::club as read};
use serde::Serialize;
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

/// A rider waiting for a pair partner who isn't linked yet (Paaranfrage).
#[derive(Debug, Serialize, ToSchema)]
pub struct PairRequest {
    pub rider_id: RiderId,
    /// The partner the club entered, if any.
    pub partner_name: Option<String>,
}

impl From<read::PairRequest> for PairRequest {
    fn from(request: read::PairRequest) -> Self {
        Self {
            rider_id: request.rider_id,
            partner_name: request.partner_name,
        }
    }
}

/// The pair requests of all clubs, to link pairs across clubs.
#[utoipa::path(
    get,
    tags=["query"],
    path="/list_pair_requests",
    responses(
        (status=200, content_type="application/json", body=Vec<PairRequest>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_pair_requests(
    Extension(db): Extension<DbHandle>,
    auth: Granted<permit::ViewRiders>,
) -> Result<Json<Vec<PairRequest>>, HttpError> {
    Ok(Json(
        db.list_pair_requests(&auth.actor())
            .await?
            .into_iter()
            .map(PairRequest::from)
            .collect(),
    ))
}
