use crate::ids::{CategoryId, FinalId};
use axum::{Extension, Json};
use db_core::DbHandle;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    roles::permit,
    timeplan::{EntryFields, EntryKind},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct AddTimeplanEntryResponse {
    /// The new entry's id.
    id: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddTimeplanEntryBody {
    earliest_start_time: Option<String>, // ISO datetime string
    duration_seconds: Option<i64>,
    label: Option<String>,
    /// The start block's home category.
    #[serde(default)]
    category_id: Option<CategoryId>,
    /// A start block running this final instead of a category.
    #[serde(default)]
    final_id: Option<FinalId>,
    /// Without one: a start block if a category (or final) is set, otherwise
    /// a custom entry. A start block without either needs a label, e.g.
    /// "Nachstarter".
    #[serde(default)]
    kind: Option<EntryKind>,
    /// Start blocks only: overrides the home category's (or final's)
    /// Einfahrzeit.
    #[serde(default)]
    einfahrzeit_seconds: Option<i64>,
    /// Custom entries only: photos next to the clock on the info board.
    #[serde(default = "crate::utils::default_true")]
    show_photos: bool,
    /// Custom entries only: an internal timeplan entry, only for the
    /// organizer team.
    #[serde(default)]
    internal: bool,
    /// Insert after this entry; without one at the end.
    #[serde(default)]
    after_id: Option<i64>,
}

/// Add a new timeplan entry.
///
/// This endpoint is used to create a new timeplan entry: a start block or a
/// custom entry (a break, an award ceremony, …).
#[utoipa::path(
    post,
    tags=["command", "timeplan"],
    path="/add_timeplan_entry",
    request_body=AddTimeplanEntryBody,
    responses(
        (status=200, content_type="application/json", body=AddTimeplanEntryResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn add_timeplan_entry(
    Extension(repo): Extension<DbHandle>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<AddTimeplanEntryBody>,
) -> Result<Json<AddTimeplanEntryResponse>, HttpError> {
    let fields = EntryFields::new(
        body.kind,
        body.category_id,
        body.final_id,
        body.label,
        body.duration_seconds,
        body.einfahrzeit_seconds,
    )
    .and_then(|fields| {
        fields
            .with_photos(body.show_photos)
            .with_internal(body.internal)
    })
    .map_err(|e| HttpError::ErrorMessages(e.to_string()))?;
    let id = repo
        .add_timeplan_entry(&fields, body.earliest_start_time.as_deref(), body.after_id)
        .await?;

    Ok(Json(AddTimeplanEntryResponse { id }))
}
