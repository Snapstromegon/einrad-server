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
    /// Has a single, by `gender` Einzel weiblich or männlich.
    single_male: bool,
    single_female: bool,
    /// Is in a pair act or waits for a partner (a pair request).
    pair: bool,
    /// The partner of the pair act.
    partner_id: Option<Uuid>,
    /// The name of the requested partner, or of the pair act's partner.
    partner_name: Option<String>,
    resolved_partner_name: Option<String>,
    resolved_partner_club: Option<String>,
    /// Number of groups the rider is in, as a regular or replacement rider.
    group_count: i64,
    /// Starts in acts whose fee is waived. They count in the flags and
    /// `group_count` above, but cost nothing.
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
            (rider.gender = 'male' AND EXISTS (
              SELECT 1 FROM act_participants
              JOIN acts ON acts.id = act_participants.act_id
              WHERE act_participants.rider_id = rider.id AND acts.act_type = 'single'
            )) as "single_male!: bool",
            (rider.gender = 'female' AND EXISTS (
              SELECT 1 FROM act_participants
              JOIN acts ON acts.id = act_participants.act_id
              WHERE act_participants.rider_id = rider.id AND acts.act_type = 'single'
            )) as "single_female!: bool",
            (request.rider_id IS NOT NULL OR EXISTS (
              SELECT 1 FROM act_participants
              JOIN acts ON acts.id = act_participants.act_id
              WHERE act_participants.rider_id = rider.id AND acts.act_type = 'pair'
            )) as "pair!: bool",
            partner.id as "partner_id: Uuid",
            CASE WHEN partner.id IS NULL THEN request.partner_name ELSE partner.firstname || ' ' || partner.lastname END as "partner_name: String",
            NULLIF(concat_ws(" ", partner.firstname, partner.lastname), '') as "resolved_partner_name: String",
            partner_club.name as resolved_partner_club,
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
          LEFT JOIN pair_requests as request ON request.rider_id = rider.id
          LEFT JOIN rider as partner ON partner.id = (
            SELECT other.rider_id FROM act_participants own
              JOIN acts ON acts.id = own.act_id AND acts.act_type = 'pair'
              JOIN act_participants other ON other.act_id = own.act_id AND other.rider_id != own.rider_id
            WHERE own.rider_id = rider.id
          )
          LEFT JOIN clubs as partner_club ON partner_club.id = partner.club_id
        WHERE rider.club_id = ?
        "#,
        club_id
    )
    .fetch_all(&mut *connection)
    .await?;
    Ok(Json(club_riders))
}
