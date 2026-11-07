use axum::{Extension, Json, extract::Query};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    groups::ensure_club_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    jury::{self, ClubJudge, JuryEnabled},
    reloadable_sqlite::ReloadableSqlite,
};

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListClubJudgesQuery {
    club_id: Uuid,
}

/// List a club's judges with their registrations. For the club and admins.
#[utoipa::path(
    get,
    tags=["query", "jury"],
    path="/list_club_judges",
    params(ListClubJudgesQuery),
    responses(
        (status=200, content_type="application/json", body=Vec<ClubJudge>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_club_judges(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    Query(query): Query<ListClubJudgesQuery>,
) -> Result<Json<Vec<ClubJudge>>, HttpError> {
    let db = db.get().await.clone();
    let mut conn = db.acquire().await?;
    ensure_club_access(&mut conn, &auth, query.club_id).await?;
    Ok(Json(
        jury::list_judges(&mut conn, Some(query.club_id)).await?,
    ))
}
