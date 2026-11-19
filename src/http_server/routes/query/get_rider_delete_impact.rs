use crate::ids::{JudgeId, RiderId};
use axum::{Extension, Json, extract::Query};
use tracing::instrument;

use crate::{
    groups::ensure_rider_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::ReloadableSqlite,
    rider_registration,
    system_status::Capabilities,
};

/// The judge linked to the rider and what deleting them takes along.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct LinkedJudgeImpact {
    judge_id: JudgeId,
    /// Number of the judge's registrations for jury units.
    registrations: i64,
    /// Number of the judge's assignments in the jury plan.
    assignments: i64,
    /// The user may change judges now, so deleting the judge with the rider
    /// is possible.
    can_delete_judge: bool,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct RiderDeleteImpact {
    /// Null if no judge is linked to the rider.
    linked_judge: Option<LinkedJudgeImpact>,
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct GetRiderDeleteImpactQuery {
    rider_id: RiderId,
}

/// What deleting a rider affects besides their starts: the linked judge.
///
/// Only for the rider's club and admins.
#[utoipa::path(
    get,
    tags=["query", "club"],
    params(GetRiderDeleteImpactQuery),
    path="/get_rider_delete_impact",
    responses(
        (status=200, content_type="application/json", body=RiderDeleteImpact),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn get_rider_delete_impact(
    Extension(db): Extension<ReloadableSqlite>,
    Query(query): Query<GetRiderDeleteImpactQuery>,
    auth: Auth,
    capabilities: Capabilities,
) -> Result<Json<RiderDeleteImpact>, HttpError> {
    let db = db.get().await.clone();
    let mut connection = db.acquire().await?;
    ensure_rider_access(&mut connection, &auth, query.rider_id).await?;
    let linked_judge = rider_registration::linked_judge_counts(&mut connection, query.rider_id)
        .await?
        .map(|judge| LinkedJudgeImpact {
            judge_id: judge.judge_id,
            registrations: judge.registrations,
            assignments: judge.assignments,
            can_delete_judge: capabilities.can_register_judge,
        });
    Ok(Json(RiderDeleteImpact { linked_judge }))
}
