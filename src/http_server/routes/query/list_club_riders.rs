use crate::ids::ClubId;
use crate::{gender::Gender, ids::RiderId};
use axum::{Extension, Json, extract::Query};
use db_core::{
    prelude::*,
    queries::club::{self as read, ListClubRiders},
};
use serde::Serialize;
use tracing::instrument;
use utoipa::ToSchema;

use crate::http_server::{ClientError, HttpError, extractor::auth::Auth};

/// A rider as their own club sees them.
#[derive(Debug, Serialize, ToSchema)]
pub struct ClubRider {
    pub id: RiderId,
    pub club_id: ClubId,
    pub firstname: String,
    pub lastname: String,
    #[serde(with = "time::serde::iso8601")]
    pub birthdate: time::OffsetDateTime,
    /// Decides whether the single is Einzel weiblich or männlich. Missing for
    /// riders registered before it was asked.
    pub gender: Option<Gender>,
    /// Number of groups the rider is in, as a regular or replacement rider.
    pub group_count: i64,
    /// Starts in acts whose fee is waived. They cost nothing.
    pub waived_count: i64,
    /// The rider's own starts are all free (a rider's fee waiver).
    pub fee_waived: bool,
}

impl From<read::ClubRider> for ClubRider {
    fn from(rider: read::ClubRider) -> Self {
        Self {
            id: rider.id,
            club_id: rider.club_id,
            firstname: rider.firstname,
            lastname: rider.lastname,
            birthdate: rider.birthdate,
            gender: rider.gender,
            group_count: rider.group_count,
            waived_count: rider.waived_count,
            fee_waived: rider.fee_waived,
        }
    }
}

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
        db.list_club_riders(
            &auth.actor(),
            ListClubRiders {
                club_id: query.club_id,
            },
        )
        .await?
        .into_iter()
        .map(ClubRider::from)
        .collect(),
    ))
}
