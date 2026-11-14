use axum::{Extension, Json};
use db_core::DbHandle;
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    images::CompetitionImages,
    roles::permit,
};

/// The competition's images and where they come from, with the failed
/// downloads to warn about (#291). Admin.
#[utoipa::path(
    get,
    tags=["query", "competition"],
    path="/get_competition_images",
    responses(
        (status=200, content_type="application/json", body=CompetitionImages),
        (status=403, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn get_competition_images(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageCompetition>,
) -> Result<Json<CompetitionImages>, HttpError> {
    Ok(Json(CompetitionImages::load(repo.as_ref()).await?))
}
