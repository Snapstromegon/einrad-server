use crate::ids::RiderId;
use axum::{Extension, Json};
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct CheckIn {
    rider_id: RiderId,
    /// `null` if the rider hasn't checked in yet.
    #[serde(with = "time::serde::iso8601::option")]
    checked_in_at: Option<time::OffsetDateTime>,
}

/// The check-in state of every rider. Check-in and music control.
///
/// Small enough to poll during the competition.
#[utoipa::path(
    get,
    tags=["query", "check-in"],
    path="/list_check_ins",
    responses(
        (status=200, content_type="application/json", body=Vec<CheckIn>),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_check_ins(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::CheckIn>,
) -> Result<Json<Vec<CheckIn>>, HttpError> {
    let db = db.get().await.clone();
    let check_ins = sqlx::query_as!(
        CheckIn,
        r#"
        SELECT
            id as "rider_id!: RiderId",
            checked_in_at as "checked_in_at: time::OffsetDateTime"
        FROM rider
        "#
    )
    .fetch_all(&db)
    .await?;
    Ok(Json(check_ins))
}
