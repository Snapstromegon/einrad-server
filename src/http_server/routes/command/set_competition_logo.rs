use axum::{Extension, Json, extract::Multipart, http::StatusCode};
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    competition::Competition,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    logo::{self, Logo},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetCompetitionLogoResponse {}

/// The multipart body: the logo as its only file.
#[derive(ToSchema, Debug)]
pub struct LogoUpload {
    #[allow(dead_code)]
    #[schema(value_type = String, format = Binary)]
    pub file: Vec<u8>,
}

/// Upload the competition's logo.
///
/// Replaces an earlier logo (#286). The format comes from the content: PNG,
/// JPEG, WebP or SVG, up to 1 MiB. Raster logos are scaled down to 512 px on
/// the long edge and lose their metadata; SVG logos are stored as they are,
/// but refused if they could run scripts or load other files (see
/// `crate::logo`). Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/set_competition_logo",
    request_body=LogoUpload,
    responses(
        (status=200, content_type="application/json", body=SetCompetitionLogoResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=413, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, body))]
#[axum::debug_handler]
pub async fn set_competition_logo(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageCompetition>,
    mut body: Multipart,
) -> Result<Json<SetCompetitionLogoResponse>, HttpError> {
    let field = body
        .next_field()
        .await
        .map_err(|e| HttpError::StatusCode(e.status()))?
        .ok_or(HttpError::StatusCode(StatusCode::BAD_REQUEST))?;
    // A body over the request limit fails here, as 413.
    let bytes = field
        .bytes()
        .await
        .map_err(|e| HttpError::StatusCode(e.status()))?;
    // Scaling a raster logo takes a moment.
    let logo = tokio::task::spawn_blocking(move || Logo::from_upload(&bytes))
        .await
        .map_err(|_| HttpError::InternalServerError)?
        .map_err(|e| HttpError::ErrorMessages(e.to_string()))?;
    let db = db.get().await.clone();
    let competition = Competition::current(&db)
        .await?
        .ok_or(HttpError::NotFound)?;
    logo::store(&db, &competition.id, &logo).await?;
    info!(
        "Set the logo of competition {} ({})",
        competition.id,
        logo.format.name()
    );
    Ok(Json(SetCompetitionLogoResponse {}))
}
