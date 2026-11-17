use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReorderSponsorsBody {
    /// All sponsors' ids in the new order.
    pub ids: Vec<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReorderSponsorsResponse {}

/// Set the order of the sponsors in the info board's footer.
///
/// `ids` must be all sponsors, each once (#291). Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/reorder_sponsors",
    request_body=ReorderSponsorsBody,
    responses(
        (status=200, content_type="application/json", body=ReorderSponsorsResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn reorder_sponsors(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageCompetition>,
    Json(body): Json<ReorderSponsorsBody>,
) -> Result<Json<ReorderSponsorsResponse>, HttpError> {
    if !repo.reorder_sponsors(&body.ids).await? {
        return Err(HttpError::ErrorMessages(
            "Die Reihenfolge muss alle Sponsoren genau einmal enthalten.".to_string(),
        ));
    }
    info!("Reordered the sponsors");
    Ok(Json(ReorderSponsorsResponse {}))
}
