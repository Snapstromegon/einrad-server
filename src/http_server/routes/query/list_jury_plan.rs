use crate::ids::ClubId;
use axum::{Extension, Json, extract::Query};
use db_core::{prelude::*, queries::jury::ListJuryPlan};
use time::OffsetDateTime;
use tracing::instrument;

use crate::{
    http_server::{
        ClientError, HttpError, extractor::auth::Auth, routes::http_types::JuryAssignment,
    },
    jury::JuryEnabled,
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
    assignments: Vec<JuryAssignment>,
}

/// The Juryplan. Admins see all of it; a club sees its own judges'
/// assignments once the plan is published.
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
#[instrument(skip(repo))]
pub async fn list_jury_plan(
    _jury: JuryEnabled,
    Extension(repo): Extension<DbHandle>,
    auth: Auth,
    Query(query): Query<ListJuryPlanQuery>,
) -> Result<Json<JuryPlan>, HttpError> {
    let plan = repo
        .list_jury_plan(
            &auth.actor(),
            ListJuryPlan {
                club_id: query.club_id,
            },
        )
        .await?;
    Ok(Json(JuryPlan {
        published_at: plan.published_at,
        assignments: plan.assignments.into_iter().map(Into::into).collect(),
    }))
}
