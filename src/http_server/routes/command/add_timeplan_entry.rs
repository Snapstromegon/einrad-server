use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    finals,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
    timeplan::{EntryFields, EntryKind, entry_ids, set_entry_order},
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
    category: Option<String>,
    /// A start block running this final instead of a category.
    #[serde(default)]
    final_name: Option<String>,
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
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn add_timeplan_entry(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<AddTimeplanEntryBody>,
) -> Result<Json<AddTimeplanEntryResponse>, HttpError> {
    let fields = EntryFields::new(
        body.kind,
        body.category,
        body.final_name,
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
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;
    if let Some(final_name) = &fields.final_name {
        finals::check_plannable(&mut tx, final_name, None)
            .await
            .map_err(HttpError::ErrorMessages)?;
    }

    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO timeplan (
            earliest_start_time,
            duration_seconds,
            label,
            category,
            final_name,
            kind,
            einfahrzeit_seconds,
            show_photos,
            internal,
            position
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, (SELECT COALESCE(MAX(position), 0) + 1 FROM timeplan))
        RETURNING id as "id!"
        "#,
        body.earliest_start_time,
        fields.duration_seconds,
        fields.label,
        fields.category,
        fields.final_name,
        fields.kind,
        fields.einfahrzeit_seconds,
        fields.show_photos,
        fields.internal,
    )
    .fetch_one(&mut *tx)
    .await?;

    if let Some(after_id) = body.after_id {
        let mut ids = entry_ids(&mut tx).await?;
        ids.retain(|&e| e != id);
        let index = ids
            .iter()
            .position(|&e| e == after_id)
            .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;
        ids.insert(index + 1, id);
        set_entry_order(&mut tx, &ids).await?;
    }
    tx.commit().await?;

    Ok(Json(AddTimeplanEntryResponse { id }))
}
