use axum::{Extension, Json};
use db_core::DbHandle;
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::{ClubJudge, JuryEnabled},
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
#[instrument(skip(repo))]
pub async fn list_judges(
    _jury: JuryEnabled,
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageJury>,
) -> Result<Json<Vec<ClubJudge>>, HttpError> {
    Ok(Json(repo.list_judges(None).await?))
}
