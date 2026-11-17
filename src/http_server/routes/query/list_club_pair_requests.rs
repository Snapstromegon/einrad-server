use crate::ids::ClubId;
use axum::{Extension, Json, extract::Query};
use db_core::DbHandle;
use tracing::instrument;

use super::list_pair_requests::PairRequest;
use crate::http_server::{ClientError, HttpError, extractor::auth::Auth};

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListClubPairRequestsQuery {
    club_id: ClubId,
}

/// The pair requests of a club's riders.
///
/// Only for the club and admins.
#[utoipa::path(
    get,
    tags=["query", "club"],
    params(ListClubPairRequestsQuery),
    path="/list_club_pair_requests",
    responses(
        (status=200, content_type="application/json", body=Vec<PairRequest>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_club_pair_requests(
    Extension(db): Extension<DbHandle>,
    Query(query): Query<ListClubPairRequestsQuery>,
    auth: Auth,
) -> Result<Json<Vec<PairRequest>>, HttpError> {
    Ok(Json(
        db.list_club_pair_requests(&auth.actor(), query.club_id)
            .await?,
    ))
}
