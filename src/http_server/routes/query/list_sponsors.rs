use axum::{Extension, Json};
use db_core::DbHandle;
use serde::Serialize;
use tracing::instrument;
use utoipa::ToSchema;

use crate::http_server::{ClientError, HttpError};

#[derive(Debug, Serialize, ToSchema)]
pub struct Sponsor {
    pub id: i64,
    /// The image's alt text.
    pub name: String,
    pub link: Option<String>,
    /// The image is at `/api/competition/sponsors/{id}/image?v={version}`;
    /// the version changes with the image.
    pub version: i64,
}

/// The sponsors for the info board's footer, in order (#291). Public. A
/// sponsor without an image isn't listed.
#[utoipa::path(
    get,
    tags=["query", "competition"],
    path="/list_sponsors",
    responses(
        (status=200, content_type="application/json", body=Vec<Sponsor>),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn list_sponsors(
    Extension(repo): Extension<DbHandle>,
) -> Result<Json<Vec<Sponsor>>, HttpError> {
    Ok(Json(
        repo.list_images()
            .await?
            .into_iter()
            .filter(|image| image.kind == "sponsor" && image.has_data)
            .map(|image| Sponsor {
                id: image.id,
                name: image.name,
                link: image.link,
                version: image.updated_at.unix_timestamp_nanos() as i64 / 1_000_000,
            })
            .collect(),
    ))
}
