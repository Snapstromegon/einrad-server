use crate::{
    gender::Gender,
    ids::{ClubId, RiderId},
};
use axum::{Extension, Json};
use db_core::{prelude::*, queries::club as read};
use serde::Serialize;
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

/// A rider with their club and starts.
#[derive(Debug, Serialize, ToSchema)]
pub struct Rider {
    pub id: RiderId,
    pub club_id: ClubId,
    pub club_name: String,
    pub firstname: String,
    pub lastname: String,
    /// Only for who may see birthdates.
    #[serde(with = "time::serde::iso8601::option")]
    pub birthdate: Option<time::OffsetDateTime>,
    pub gender: Option<Gender>,
    /// On the determining day; only for who may see birthdates.
    pub age_on_competition: Option<f64>,
    /// When the rider checked in on competition day.
    #[serde(with = "time::serde::iso8601::option")]
    pub checked_in_at: Option<time::OffsetDateTime>,
    /// Number of groups the rider is in, as a regular or replacement rider.
    pub group_count: i64,
    /// Starts in acts whose fee is waived. They cost nothing.
    pub waived_count: i64,
    /// The rider's own starts are all free (a rider's fee waiver).
    pub fee_waived: bool,
    /// Why the rider's fees are waived.
    pub fee_waiver: Option<String>,
}

impl From<read::Rider> for Rider {
    fn from(rider: read::Rider) -> Self {
        Self {
            id: rider.id,
            club_id: rider.club_id,
            club_name: rider.club_name,
            firstname: rider.firstname,
            lastname: rider.lastname,
            birthdate: rider.birthdate,
            gender: rider.gender,
            age_on_competition: rider.age_on_competition,
            checked_in_at: rider.checked_in_at,
            group_count: rider.group_count,
            waived_count: rider.waived_count,
            fee_waived: rider.fee_waived,
            fee_waiver: rider.fee_waiver,
        }
    }
}

/// Every rider with their club and starts; birthdates and ages only for
/// who may see them.
#[utoipa::path(
    get,
    tags=["query"],
    path="/list_riders",
    responses(
        (status=200, content_type="application/json", body=Vec<Rider>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_riders(
    Extension(db): Extension<DbHandle>,
    auth: Granted<permit::ViewRiders>,
    birthdates: Option<Granted<permit::ViewBirthdates>>,
) -> Result<Json<Vec<Rider>>, HttpError> {
    let club_riders = db
        .list_riders(&auth.actor())
        .await?
        .into_iter()
        .map(Rider::from)
        .map(|rider| {
            if birthdates.is_some() {
                rider
            } else {
                Rider {
                    birthdate: None,
                    age_on_competition: None,
                    ..rider
                }
            }
        })
        .collect();
    Ok(Json(club_riders))
}
