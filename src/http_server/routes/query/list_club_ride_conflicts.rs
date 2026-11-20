use crate::ids::ClubId;
use axum::{Extension, Json, extract::Query};
use tracing::instrument;

use crate::{
    groups::ensure_club_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    jury::{self, JuryEnabled, RiderRideConflict},
    reloadable_sqlite::ReloadableSqlite,
};

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListClubRideConflictsQuery {
    club_id: ClubId,
}

/// List the heats the riders of a club ride in (also as replacement, and as
/// members of another club's group), one row per rider and heat. A linked
/// judge can't be newly registered for those Vorläufe, so the jury dialog
/// shows them as unavailable. For the club and admins.
#[utoipa::path(
    get,
    tags=["query", "jury"],
    path="/list_club_ride_conflicts",
    params(ListClubRideConflictsQuery),
    responses(
        (status=200, content_type="application/json", body=Vec<RiderRideConflict>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_club_ride_conflicts(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    Query(query): Query<ListClubRideConflictsQuery>,
) -> Result<Json<Vec<RiderRideConflict>>, HttpError> {
    let db = db.get().await.clone();
    let mut conn = db.acquire().await?;
    ensure_club_access(&mut conn, &auth, query.club_id).await?;
    Ok(Json(
        jury::list_ride_conflicts(&mut conn, query.club_id).await?,
    ))
}
