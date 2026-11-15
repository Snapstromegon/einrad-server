use crate::ids::{CategoryId, FinalId};
use axum::{Extension, Json};
use db_core::{commands::timeplan::EditTimeplanEntry, prelude::*};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    http_server::{
        ClientError, HttpError, extractor::auth::Granted, routes::http_types::EntryKind,
    },
    roles::permit,
    timeplan::EntryFields,
    utils::parse_start_time,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditTimeplanEntryResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditTimeplanEntryBody {
    id: i64,                             // Primary key - timeplan entry to edit
    earliest_start_time: Option<String>, // RFC 3339 datetime string
    duration_seconds: Option<i64>,
    label: Option<String>,
    /// The start block's home category.
    #[serde(default)]
    category_id: Option<CategoryId>,
    /// A start block running this final instead of a category.
    #[serde(default)]
    final_id: Option<FinalId>,
    /// Without one: a start block if a category (or final) is set, otherwise
    /// a custom entry. Turning a start block into a custom entry or a final's
    /// block drops its order.
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
}

/// Edit an existing timeplan entry.
///
/// This endpoint is used to update a timeplan entry's properties. Every field
/// is overwritten.
#[utoipa::path(
    post,
    tags=["command", "timeplan"],
    path="/edit_timeplan_entry",
    request_body=EditTimeplanEntryBody,
    responses(
        (status=200, content_type="application/json", body=EditTimeplanEntryResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
#[axum::debug_handler]
pub async fn edit_timeplan_entry(
    Extension(repo): Extension<DbHandle>,
    auth: Granted<permit::PlanCompetition>,
    Json(body): Json<EditTimeplanEntryBody>,
) -> Result<Json<EditTimeplanEntryResponse>, HttpError> {
    let fields = EntryFields::new(
        body.kind.map(Into::into),
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
    let earliest_start_time = parse_start_time(body.earliest_start_time.as_deref())?;
    repo.edit_timeplan_entry(
        &auth.actor(),
        EditTimeplanEntry {
            id: body.id,
            fields,
            earliest_start_time,
        },
    )
    .await?;

    Ok(Json(EditTimeplanEntryResponse {}))
}
