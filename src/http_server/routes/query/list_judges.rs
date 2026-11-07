use axum::{Extension, Json};
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::{self, ClubJudge, JuryEnabled},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

/// List the judges of all clubs, by club. Jury lead.
#[utoipa::path(
    get,
    tags=["query", "jury"],
    path="/list_judges",
    responses(
        (status=200, content_type="application/json", body=Vec<ClubJudge>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
pub async fn list_judges(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
) -> Result<Json<Vec<ClubJudge>>, HttpError> {
    let db = db.get().await.clone();
    let mut conn = db.acquire().await?;
    Ok(Json(jury::list_judges(&mut conn, None).await?))
}
