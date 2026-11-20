use axum::{Extension, Json};
use tracing::instrument;

use crate::{
    http_server::{
        ClientError, HttpError,
        extractor::jury_plan::JuryPlanAccess,
    },
    jury::{self, JuryAssignment},
    reloadable_sqlite::ReloadableSqlite,
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct PublicJuryAssignment {
    pub judge_name: String,
    pub assignment: JuryAssignment,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct PublicJuryPlan {
    #[serde(with = "time::serde::rfc3339::option")]
    pub published_at: Option<time::OffsetDateTime>,
    pub assignments: Vec<PublicJuryAssignment>,
}

/// The public, complete Juryplan.
#[utoipa::path(
    get,
    tags=["query", "jury"],
    path="/juryplan",
    responses(
        (status=200, content_type="application/json", body=PublicJuryPlan),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn juryplan(
    _: JuryPlanAccess,
    Extension(db): Extension<ReloadableSqlite>,
) -> Result<Json<PublicJuryPlan>, HttpError> {
    let db = db.get().await.clone();
    let mut conn = db.acquire().await?;
    let published_at = jury::plan_published_at(&mut conn).await?;
    let judges = jury::list_judges(&mut conn, None).await?;
    drop(conn);
    let assignments = jury::plan_assignments(&db, None).await?;
    let names = judges
        .into_iter()
        .map(|judge| (judge.id, format!("{} {}", judge.firstname, judge.lastname)))
        .collect::<std::collections::HashMap<_, _>>();
    let assignments = assignments
        .into_iter()
        .map(|assignment| PublicJuryAssignment {
            judge_name: names
                .get(&assignment.judge_id)
                .cloned()
                .unwrap_or_else(|| "?".to_string()),
            assignment,
        })
        .collect();
    Ok(Json(PublicJuryPlan {
        published_at,
        assignments,
    }))
}
