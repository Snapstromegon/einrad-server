use crate::ids::{ClubId, RiderId};
use axum::{Extension, Json, extract::Query};
use tracing::instrument;

use super::list_pair_requests::PairRequest;
use crate::{
    groups::ensure_club_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::ReloadableSqlite,
};

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
    Extension(db): Extension<ReloadableSqlite>,
    Query(query): Query<ListClubPairRequestsQuery>,
    auth: Auth,
) -> Result<Json<Vec<PairRequest>>, HttpError> {
    let db = db.get().await.clone();
    let mut connection = db.acquire().await?;
    ensure_club_access(&mut connection, &auth, query.club_id).await?;
    let requests = sqlx::query_as!(
        PairRequest,
        r#"
        SELECT pair_requests.rider_id as "rider_id!: RiderId", pair_requests.partner_name
        FROM pair_requests JOIN rider ON rider.id = pair_requests.rider_id
        WHERE rider.club_id = $1
        "#,
        query.club_id
    )
    .fetch_all(&mut *connection)
    .await?;
    Ok(Json(requests))
}
