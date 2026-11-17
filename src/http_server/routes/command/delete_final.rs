use crate::ids::FinalId;
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
pub struct DeleteFinalResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteFinalBody {
    id: FinalId,
}

/// Delete a final, with its slots and its timeplan block.
///
/// Not possible once the final has started.
#[utoipa::path(
    post,
    tags=["command", "final"],
    path="/delete_final",
    request_body=DeleteFinalBody,
    responses(
        (status=200, content_type="application/json", body=DeleteFinalResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=409, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn delete_final(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<DeleteFinalBody>,
) -> Result<Json<DeleteFinalResponse>, HttpError> {
    repo.delete_final(body.id).await?;
    Ok(Json(DeleteFinalResponse {}))
}
