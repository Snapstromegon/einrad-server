use axum::{Extension, Json};
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::jury_plan::JuryPlanAccess},
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
    pub sessions: Vec<PublicJurySession>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct PublicJurySession {
    pub unit_id: crate::ids::JuryUnitId,
    pub session_id: crate::ids::JurySessionId,
    pub unit_name: String,
    pub session_label: String,
    pub needed_technical: i64,
    pub needed_performance: i64,
    pub needed_dismounts: i64,
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
    let blocks = jury::predicted_blocks(&db).await;
    let units = jury::list_units_in(&db, &blocks).await?;
    let sessions = units
        .into_iter()
        .flat_map(|unit| {
            let unit_id = unit.id;
            let unit_name = unit.name;
            let split = unit.sessions.len() > 1;
            unit.sessions.into_iter().map(move |session| {
                let session_label = if split {
                    session
                        .categories
                        .iter()
                        .map(|category| category.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                } else {
                    String::new()
                };
                PublicJurySession {
                    unit_id,
                    session_id: session.id,
                    unit_name: unit_name.clone(),
                    session_label,
                    needed_technical: session.needed_technical,
                    needed_performance: session.needed_performance,
                    needed_dismounts: session.needed_dismounts,
                }
            })
        })
        .collect();
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
        sessions,
    }))
}
