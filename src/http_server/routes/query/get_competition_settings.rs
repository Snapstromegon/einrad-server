use std::sync::Arc;

use axum::{Extension, Json};
use serde::Serialize;
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    competition::{Competition, Features, ResolvedFeatures},
    http_server::{ClientError, HttpError, HttpServerOptions, extractor::auth::Granted},
    photos::PhotosSettings,
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct CompetitionSettings {
    pub competition: Competition,
    /// Where the info board's photos come from, with the password.
    pub photos: Option<PhotosSettings>,
    /// What the features the competition leaves unset are: the system
    /// config's `[features]`, else the build default.
    pub feature_defaults: ResolvedFeatures,
    /// The id in the competition file the server started with, if it
    /// differs from the competition's: that file is ignored.
    pub file_id_mismatch: Option<String>,
}

/// The competition's settings to edit, with the photo password. Admin.
#[utoipa::path(
    get,
    tags=["query", "competition"],
    path="/get_competition_settings",
    responses(
        (status=200, content_type="application/json", body=CompetitionSettings),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, http_options))]
pub async fn get_competition_settings(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    _: Granted<permit::ManageCompetition>,
) -> Result<Json<CompetitionSettings>, HttpError> {
    let db = db.get().await.clone();
    let mut competition = Competition::current(&db)
        .await?
        .ok_or(HttpError::NotFound)?;
    Ok(Json(CompetitionSettings {
        photos: competition.photos.take(),
        feature_defaults: Features::default().resolve(&http_options.features),
        file_id_mismatch: http_options
            .competition_file_id
            .clone()
            .filter(|id| *id != competition.id),
        competition,
    }))
}
