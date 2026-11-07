use axum::{Extension, Json};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

/// A rider waiting for a pair partner who isn't linked yet (Paaranfrage).
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct PairRequest {
    pub(super) rider_id: Uuid,
    /// The partner the club entered, if any.
    pub(super) partner_name: Option<String>,
}

/// The pair requests of all clubs, to link pairs across clubs.
#[utoipa::path(
    get,
    tags=["query"],
    path="/list_pair_requests",
    responses(
        (status=200, content_type="application/json", body=Vec<PairRequest>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_pair_requests(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ViewRiders>,
) -> Result<Json<Vec<PairRequest>>, HttpError> {
    let db = db.get().await.clone();
    let requests = sqlx::query_as!(
        PairRequest,
        r#"SELECT rider_id as "rider_id!: Uuid", partner_name FROM pair_requests"#
    )
    .fetch_all(&db)
    .await?;
    Ok(Json(requests))
}
