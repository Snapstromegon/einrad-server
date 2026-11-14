use crate::ids::ClubId;
use axum::{Extension, Json, extract::Query};
use db_core::DbHandle;
use tracing::instrument;

use crate::http_server::{ClientError, HttpError, extractor::auth::Auth};

pub use db_core::riders::ClubRider;

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListClubRidersQuery {
    club_id: ClubId,
}

/// List the riders of a club, with their birthdates.
///
/// Only for the club and admins.
#[utoipa::path(
    get,
    tags=["query", "club"],
    params(ListClubRidersQuery),
    path="/list_club_riders",
    responses(
        (status=200, content_type="application/json", body=Vec<ClubRider>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_club_riders(
    Extension(db): Extension<DbHandle>,
    Query(query): Query<ListClubRidersQuery>,
    auth: Auth,
) -> Result<Json<Vec<ClubRider>>, HttpError> {
    Ok(Json(
        db.list_club_riders(&auth.actor(), query.club_id).await?,
    ))
}
