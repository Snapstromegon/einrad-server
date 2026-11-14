use std::sync::Arc;

use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use super::image_support::{apply_source, svg_logo};
use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    image_fetch::ImageFetcher,
    images::{self, Slot},
    roles::permit,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddSponsorBody {
    /// The label in the admin UI and the image's alt text.
    pub name: String,
    /// Where the logo leads; the info board doesn't show it.
    pub link: Option<String>,
    /// A sponsor logo built into the server (`gwn`, `heiner_weiss`), as the
    /// image; or
    pub preset: Option<String>,
    /// an inline SVG; or
    pub svg: Option<String>,
    /// a URL to download the image from. Without any of the three, the
    /// image is uploaded afterwards.
    pub url: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AddSponsorResponse {
    pub id: i64,
}

/// Add a sponsor at the end of the info board's footer.
///
/// A sponsor without an image isn't shown (#291). Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/add_sponsor",
    request_body=AddSponsorBody,
    responses(
        (status=200, content_type="application/json", body=AddSponsorResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo, fetcher, body))]
#[axum::debug_handler]
pub async fn add_sponsor(
    Extension(repo): Extension<DbHandle>,
    Extension(fetcher): Extension<Arc<ImageFetcher>>,
    _: Granted<permit::ManageCompetition>,
    Json(body): Json<AddSponsorBody>,
) -> Result<Json<AddSponsorResponse>, HttpError> {
    let name = images::clean_name(&body.name).map_err(HttpError::ErrorMessages)?;
    let link = images::clean_link(body.link.as_deref()).map_err(HttpError::ErrorMessages)?;
    // Checked before anything is stored.
    let preset = match body.preset.as_deref() {
        None => None,
        Some(id) => Some(
            images::preset(id)
                .ok_or_else(|| HttpError::ErrorMessages(format!("Unbekanntes Preset {id}.")))?,
        ),
    };
    let preset_logo = preset.map(|preset| svg_logo(preset.svg)).transpose()?;
    let has_source = body.svg.as_deref().is_some_and(|s| !s.trim().is_empty())
        || body.url.as_deref().is_some_and(|u| !u.trim().is_empty());
    if preset.is_some() && has_source {
        return Err(HttpError::ErrorMessages(
            "Bitte nur ein Preset, ein SVG oder eine URL angeben.".to_string(),
        ));
    }
    if has_source {
        // Checked before the sponsor exists.
        match (body.svg.as_deref(), body.url.as_deref()) {
            (Some(svg), None) if !svg.trim().is_empty() => {
                svg_logo(svg)?;
            }
            (None, Some(url)) if !url.trim().is_empty() => {
                super::image_support::image_url(url)?;
            }
            _ => {
                return Err(HttpError::ErrorMessages(
                    "Bitte entweder ein SVG oder eine URL angeben.".to_string(),
                ));
            }
        }
    }

    let id = repo
        .add_sponsor(&name, link.as_deref())
        .await?
        .ok_or(HttpError::NotFound)?;
    if let Some(logo) = preset_logo {
        repo.set_image_data(Slot::Sponsor(id), &logo).await?;
    } else if has_source {
        apply_source(
            &repo,
            &fetcher,
            Slot::Sponsor(id),
            body.svg.as_deref(),
            body.url.as_deref(),
        )
        .await?;
    }
    info!("Added sponsor {id} ({name})");
    Ok(Json(AddSponsorResponse { id }))
}
