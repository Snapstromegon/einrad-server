use crate::ids::FinalId;
use axum::{Extension, Json};
use db_core::{
    DbHandle,
    finals::{FinalChange, FinalFields},
};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    finals, heats,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditFinalResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditFinalBody {
    /// The final to edit.
    id: FinalId,
    new_name: String,
    /// The new short code; without, it stays.
    #[serde(default)]
    new_code: Option<String>,
    #[serde(flatten)]
    fields: FinalFields,
}

/// Edit a final.
///
/// Every field is overwritten. Its slots are only created anew when the slot
/// groups change, which isn't possible once the final has started; a slot
/// with the same heat and number keeps its placed act. Every heat shares the
/// final's discipline.
#[utoipa::path(
    post,
    tags=["command", "final"],
    path="/edit_final",
    request_body=EditFinalBody,
    responses(
        (status=200, content_type="application/json", body=EditFinalResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=409, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn edit_final(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<EditFinalBody>,
) -> Result<Json<EditFinalResponse>, HttpError> {
    let new_name = finals::normalize_name(&body.new_name).map_err(HttpError::ErrorMessages)?;
    let fields = body.fields.normalize().map_err(HttpError::ErrorMessages)?;
    // Without a new code the final keeps its own.
    let new_code = body
        .new_code
        .as_deref()
        .filter(|code| !code.trim().is_empty())
        .map(|code| heats::normalize_code(Some(code), &new_name));
    repo.edit_final(&FinalChange {
        id: body.id,
        new_name,
        new_code,
        fields,
    })
    .await?;
    Ok(Json(EditFinalResponse {}))
}
