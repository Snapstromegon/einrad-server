use axum::{Extension, Json, extract::Query};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    gender::Gender,
    groups::ensure_club_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::ReloadableSqlite,
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ClubRider {
    id: Uuid,
    club_id: Uuid,
    firstname: String,
    lastname: String,
    #[serde(with = "time::serde::iso8601")]
    birthdate: time::OffsetDateTime,
    /// Decides whether the single is Einzel weiblich or männlich. Missing for
    /// riders registered before it was asked.
    gender: Option<Gender>,
    /// Number of groups the rider is in, as a regular or replacement rider.
    group_count: i64,
    /// Starts in acts whose fee is waived. They cost nothing.
    waived_count: i64,
    /// The rider's own starts are all free (a rider's fee waiver).
    fee_waived: bool,
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListClubRidersQuery {
    club_id: Uuid,
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
    Extension(db): Extension<ReloadableSqlite>,
    Query(query): Query<ListClubRidersQuery>,
    auth: Auth,
) -> Result<Json<Vec<ClubRider>>, HttpError> {
    let db = db.get().await.clone();
    let mut connection = db.acquire().await?;
    let club_id = query.club_id;
    ensure_club_access(&mut connection, &auth, club_id).await?;
    let club_riders = sqlx::query_as!(
        ClubRider,
        r#"
        SELECT
            rider.id as "id!: Uuid",
            rider.club_id as "club_id!: Uuid",
            rider.firstname,
            rider.lastname,
            rider.birthdate,
            rider.gender as "gender: Gender",
            (
              SELECT COUNT(*) FROM act_participants
              JOIN acts ON acts.id = act_participants.act_id
              WHERE act_participants.rider_id = rider.id
                AND acts.act_type IN ('group_small', 'group_large')
            ) as "group_count!: i64",
            (
              SELECT COUNT(*) FROM act_participants
              JOIN acts ON acts.id = act_participants.act_id
              WHERE act_participants.rider_id = rider.id
                AND acts.fee_waiver IS NOT NULL
            ) as "waived_count!: i64",
            rider.fee_waiver IS NOT NULL as "fee_waived!: bool"
        FROM rider
        WHERE rider.club_id = ?
        "#,
        club_id
    )
    .fetch_all(&mut *connection)
    .await?;
    Ok(Json(club_riders))
}
