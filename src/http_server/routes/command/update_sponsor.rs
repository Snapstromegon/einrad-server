use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    images,
    roles::permit,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateSponsorBody {
    pub id: i64,
    pub name: String,
    pub link: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UpdateSponsorResponse {}

/// Change a sponsor's name and link (#291). Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/update_sponsor",
    request_body=UpdateSponsorBody,
    responses(
        (status=200, content_type="application/json", body=UpdateSponsorResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn update_sponsor(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::ManageCompetition>,
    Json(body): Json<UpdateSponsorBody>,
) -> Result<Json<UpdateSponsorResponse>, HttpError> {
    let name = images::clean_name(&body.name).map_err(HttpError::ErrorMessages)?;
    let link = images::clean_link(body.link.as_deref()).map_err(HttpError::ErrorMessages)?;
    if !repo.update_sponsor(body.id, &name, link.as_deref()).await? {
        return Err(HttpError::NotFound);
    }
    info!("Changed sponsor {} ({name})", body.id);
    Ok(Json(UpdateSponsorResponse {}))
}
