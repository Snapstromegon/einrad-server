use crate::ids::ActId;
use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetActOrderResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ActOrderBody {
    act_id: ActId,
    order: Option<i64>,
}

/// EditClubAct a new user.
///
/// This endpoint is used to create a new user.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/set_act_order",
    request_body=ActOrderBody,
    responses(
        (status=200, content_type="application/json", body=SetActOrderResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_act_order(
    Extension(db): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<ActOrderBody>,
) -> Result<Json<SetActOrderResponse>, HttpError> {
    db.set_act_order(body.act_id, body.order).await?;

    Ok(Json(SetActOrderResponse {}))
}
