use axum::{Extension, Json};
use tracing::instrument;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    images::CompetitionImages,
    reloadable_sqlite::ReloadableSqlite,
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
#[instrument(skip(db))]
pub async fn get_competition_images(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageCompetition>,
) -> Result<Json<CompetitionImages>, HttpError> {
    let db = db.get().await.clone();
    Ok(Json(CompetitionImages::load(&db).await?))
}
