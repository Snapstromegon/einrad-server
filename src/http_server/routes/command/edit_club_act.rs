use crate::ids::ActId;
use std::sync::Arc;

use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    competition::{Competition, Features},
    groups::ensure_act_access,
    http_server::{ClientError, HttpError, HttpServerOptions, extractor::auth::Auth},
    reloadable_sqlite::ReloadableSqlite,
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditClubActResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditClubActBody {
    id: ActId,
    name: String,
    description: Option<String>,
}

/// Edit an act's title and description.
///
/// Only clubs with a rider in the act, the club that created a group, and
/// admins can edit it.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/edit_club_act",
    request_body=EditClubActBody,
    responses(
        (status=200, content_type="application/json", body=EditClubActResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn edit_club_act(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<EditClubActBody>,
) -> Result<Json<EditClubActResponse>, HttpError> {
    if !capabilities.can_upload_music {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let db = db.get().await.clone();
    ensure_act_access(&mut *db.acquire().await?, &auth, body.id).await?;
    let moderation_input = Features::of(Competition::current(&db).await?.as_ref()).moderation_input;

    sqlx::query!(
        r#"
        UPDATE acts
        SET name = $1,
            description = CASE WHEN $2 THEN $3 ELSE description END
        WHERE id = $4
        "#,
        body.name,
        moderation_input,
        body.description,
        body.id,
    )
    .execute(&db)
    .await?;

    Ok(Json(EditClubActResponse {}))
}
