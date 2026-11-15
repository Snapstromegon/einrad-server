use axum::{Extension, Json};
use db_core::prelude::*;
use tracing::instrument;

use crate::{
    http_server::{
        ClientError, HttpError, extractor::auth::Granted, routes::http_types::ClubJudge,
    },
    jury::JuryEnabled,
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
    granted: Granted<permit::ManageJury>,
) -> Result<Json<Vec<ClubJudge>>, HttpError> {
    let judges = repo.list_judges(&granted.actor()).await?;
    Ok(Json(judges.into_iter().map(Into::into).collect()))
}
