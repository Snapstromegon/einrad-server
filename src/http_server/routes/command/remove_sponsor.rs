use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    images::{self, Slot},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct RemoveSponsorBody {
    pub id: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RemoveSponsorResponse {}

/// Remove a sponsor and its image (#291). Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/remove_sponsor",
    request_body=RemoveSponsorBody,
    responses(
        (status=200, content_type="application/json", body=RemoveSponsorResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn remove_sponsor(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageCompetition>,
    Json(body): Json<RemoveSponsorBody>,
) -> Result<Json<RemoveSponsorResponse>, HttpError> {
    let db = db.get().await.clone();
    images::remove(&db, Slot::Sponsor(body.id)).await?;
    info!("Removed sponsor {}", body.id);
    Ok(Json(RemoveSponsorResponse {}))
}
