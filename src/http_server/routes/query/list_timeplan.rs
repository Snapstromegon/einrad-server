use crate::ids::{CategoryId, FinalId};
use axum::{Extension, Json};
use db_core::{
    prelude::*,
    queries::timeplan::{ListTimeplan, TimeplanListEntry as ReadEntry},
    timeplan::Audience,
};
use tracing::instrument;

use crate::{
    http_server::{
        ClientError, HttpError,
        extractor::{auth::Granted, startlist::StartlistAccess},
        routes::http_types::EntryKind,
    },
    roles::permit,
};

/// The label an internal timeplan entry has for the public.
/// A timeplan entry as the list shows it, without its content.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct TimeplanListEntry {
    pub id: i64,
    pub kind: EntryKind,
    #[serde(with = "time::serde::iso8601::option")]
    pub earliest_start_time: Option<time::OffsetDateTime>,
    pub duration_seconds: Option<i64>,
    pub label: Option<String>,
    /// Start blocks only: the home category, by id and name.
    pub category_id: Option<CategoryId>,
    pub category: Option<String>,
    /// Start blocks only: the final running here instead of a category, by
    /// id and name.
    pub final_id: Option<FinalId>,
    pub final_name: Option<String>,
    /// Start blocks only: overrides the home category's (or final's)
    /// Einfahrzeit.
    pub einfahrzeit_seconds: Option<i64>,
    /// Custom entries: photos next to the clock on the info board.
    pub show_photos: bool,
    /// Custom entries: an internal timeplan entry. Only the organizer team
    /// gets it as it is; everyone else gets a "Pause" of the same length.
    pub internal: bool,
    /// Start blocks only: the order was edited by hand (`set_block_order`),
    /// so the category's start order no longer applies to it.
    pub manually_ordered: bool,
    #[serde(with = "time::serde::iso8601::option")]
    pub started_at: Option<time::OffsetDateTime>,
    #[serde(with = "time::serde::iso8601::option")]
    pub ended_at: Option<time::OffsetDateTime>,
}

impl From<ReadEntry> for TimeplanListEntry {
    fn from(entry: ReadEntry) -> Self {
        Self {
            id: entry.id,
            kind: entry.kind.into(),
            earliest_start_time: entry.earliest_start_time,
            duration_seconds: entry.duration_seconds,
            label: entry.label,
            category_id: entry.category_id,
            category: entry.category,
            final_id: entry.final_id,
            final_name: entry.final_name,
            einfahrzeit_seconds: entry.einfahrzeit_seconds,
            show_photos: entry.show_photos,
            internal: entry.internal,
            manually_ordered: entry.manually_ordered,
            started_at: entry.started_at,
            ended_at: entry.ended_at,
        }
    }
}

/// List all timeplan entries, in order. Before an admin published the
/// timeplan only the organizer team gets them.
#[utoipa::path(
    get,
    tags=["query"],
    path="/list_timeplan",
    responses(
        (status=200, content_type="application/json", body=Vec<TimeplanListEntry>),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo))]
pub async fn list_timeplan(
    _: StartlistAccess,
    Extension(repo): Extension<DbHandle>,
    internal: Option<Granted<permit::ViewInternalEntries>>,
) -> Result<Json<Vec<TimeplanListEntry>>, HttpError> {
    // The public gets internal entries as a "Pause", like in the prediction.
    let audience = if internal.is_none() {
        Audience::Public
    } else {
        Audience::OrganizerTeam
    };
    let actor = internal.as_ref().map(|granted| granted.actor());
    Ok(Json(
        repo.list_timeplan(actor.as_ref(), ListTimeplan { audience })
            .await?
            .into_iter()
            .map(TimeplanListEntry::from)
            .collect(),
    ))
}
