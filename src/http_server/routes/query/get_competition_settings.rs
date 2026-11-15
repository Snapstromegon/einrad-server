use std::sync::Arc;

use axum::{Extension, Json};
use db_core::prelude::*;
use serde::Serialize;
use tracing::instrument;
use utoipa::ToSchema;

use super::super::http_types::{Competition, PhotosSettings, ResolvedFeatures};
use crate::{
    http_server::{ClientError, HttpError, HttpServerOptions, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct CompetitionSettings {
    pub competition: Competition,
    /// Where the info board's photos come from, with the password.
    pub photos: Option<PhotosSettings>,
    /// What the features the competition leaves unset are: the build
    /// default.
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
#[instrument(skip(repo, http_options))]
pub async fn get_competition_settings(
    Extension(repo): Extension<DbHandle>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    auth: Granted<permit::ManageCompetition>,
) -> Result<Json<CompetitionSettings>, HttpError> {
    let mut competition = repo.get_competition_settings(&auth.actor()).await?;
    Ok(Json(CompetitionSettings {
        photos: competition.photos.take().map(Into::into),
        feature_defaults: db_core::competition::Features::default().resolve().into(),
        file_id_mismatch: http_options
            .competition_file_id
            .clone()
            .filter(|id| *id != competition.id),
        competition: competition.into(),
    }))
}
