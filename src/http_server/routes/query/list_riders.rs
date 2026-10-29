use axum::{Extension, Json};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    gender::Gender,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Rider {
    id: Uuid,
    club_id: Uuid,
    club_name: String,
    firstname: String,
    lastname: String,
    /// Only for who may see birthdates.
    #[serde(with = "time::serde::iso8601::option")]
    birthdate: Option<time::OffsetDateTime>,
    gender: Option<Gender>,
    /// On the determining day; only for who may see birthdates.
    age_on_competition: Option<f64>,
    /// When the rider checked in on competition day.
    #[serde(with = "time::serde::iso8601::option")]
    checked_in_at: Option<time::OffsetDateTime>,
    /// Number of groups the rider is in, as a regular or replacement rider.
    group_count: i64,
    /// Starts in acts whose fee is waived. They cost nothing.
    waived_count: i64,
    /// The rider's own starts are all free (a rider's fee waiver).
    fee_waived: bool,
    /// Why the rider's fees are waived.
    fee_waiver: Option<String>,
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
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ViewRiders>,
    birthdates: Option<Granted<permit::ViewBirthdates>>,
) -> Result<Json<Vec<Rider>>, HttpError> {
    let db = db.get().await.clone();
    let club_riders: Vec<Rider> = sqlx::query_as!(
        Rider,
        r#"
        SELECT
            rider.id as "id!: Uuid",
            rider.club_id as "club_id!: Uuid",
            club.name as "club_name!",
            rider.firstname,
            rider.lastname,
            rider.birthdate as "birthdate: time::OffsetDateTime",
            rider.gender as "gender: Gender",
            COALESCE(
              (
                julianday((SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1))
                - julianday(rider.birthdate)
              ) / 365.2425,
              0.0
            ) as "age_on_competition: f64",
            rider.checked_in_at as "checked_in_at: time::OffsetDateTime",
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
            rider.fee_waiver IS NOT NULL as "fee_waived!: bool",
            rider.fee_waiver
        FROM rider
          LEFT JOIN clubs as club ON club.id = rider.club_id
        "#
    )
    .fetch_all(&db)
    .await?
    .into_iter()
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
