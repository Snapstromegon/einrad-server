use crate::ids::ClubId;
use axum::{Extension, Json, extract::Query, http::StatusCode};
use time::OffsetDateTime;
use tracing::instrument;

use crate::{
    groups::ensure_club_access,
    http_server::{
        ClientError, HttpError,
        extractor::auth::{Auth, Granted},
    },
    jury::{self, JuryAssignment, JuryEnabled},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListJuryPlanQuery {
    /// Only this club's judges. Required for everyone but the jury lead.
    club_id: Option<ClubId>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct JuryPlan {
    /// When the plan was published; `None` while only the jury lead sees it.
    #[serde(with = "time::serde::rfc3339::option")]
    published_at: Option<OffsetDateTime>,
    /// One entry per assignment (per seat), sorted by time.
    assignments: Vec<JuryAssignment>,
}

/// The Juryplan. Admins see all of it; a club sees its own judges'
/// assignments once the plan is published. Every assignment carries its seat
/// and the day, block and time of its Durchgang.
#[utoipa::path(
    get,
    tags=["query", "jury"],
    path="/list_jury_plan",
    params(ListJuryPlanQuery),
    responses(
        (status=200, content_type="application/json", body=JuryPlan),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_jury_plan(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    jury_lead: Option<Granted<permit::ManageJury>>,
    Query(query): Query<ListJuryPlanQuery>,
) -> Result<Json<JuryPlan>, HttpError> {
    let db = db.get().await.clone();
    let mut conn = db.acquire().await?;
    match query.club_id {
        Some(club_id) => ensure_club_access(&mut conn, &auth, club_id).await?,
        None if jury_lead.is_some() => {}
        None => return Err(HttpError::StatusCode(StatusCode::FORBIDDEN)),
    }
    let published_at = jury::plan_published_at(&mut conn).await?;
    drop(conn);
    if published_at.is_none() && jury_lead.is_none() {
        return Ok(Json(JuryPlan {
            published_at,
            assignments: vec![],
        }));
    }
    let assignments = jury::plan_assignments(&db, query.club_id).await?;
    Ok(Json(JuryPlan {
        published_at,
        assignments,
    }))
}
