use crate::ids::ClubId;
use db_core::DbHandle;

use axum::{Extension, Json, extract::Query};
use tracing::instrument;

use crate::http_server::{ClientError, HttpError, extractor::auth::Auth};

pub use db_core::groups::ClubGroup;

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListClubGroupsQuery {
    club_id: ClubId,
}

/// List the groups of a club.
///
/// Groups the club created or has riders in, with all their members.
#[utoipa::path(
    get,
    tags=["query", "club", "group"],
    params(ListClubGroupsQuery),
    path="/list_club_groups",
    responses(
        (status=200, content_type="application/json", body=Vec<ClubGroup>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_club_groups(
    Extension(db): Extension<DbHandle>,
    Query(query): Query<ListClubGroupsQuery>,
    auth: Auth,
) -> Result<Json<Vec<ClubGroup>>, HttpError> {
    Ok(Json(
        db.list_club_groups(&auth.actor(), query.club_id).await?,
    ))
}
