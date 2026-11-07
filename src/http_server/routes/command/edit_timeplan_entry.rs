use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    finals,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
    timeplan::{EntryFields, EntryKind, Slot, load},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditTimeplanEntryResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditTimeplanEntryBody {
    id: i64,                             // Primary key - timeplan entry to edit
    earliest_start_time: Option<String>, // ISO datetime string
    duration_seconds: Option<i64>,
    label: Option<String>,
    category: Option<String>,
    /// A start block running this final instead of a category.
    #[serde(default)]
    final_name: Option<String>,
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
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn edit_timeplan_entry(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<EditTimeplanEntryBody>,
) -> Result<Json<EditTimeplanEntryResponse>, HttpError> {
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
        finals::check_plannable(&mut tx, final_name, Some(body.id))
            .await
            .map_err(HttpError::ErrorMessages)?;
    }
    // A final's block holds only the final's slots.
    let drops_slots = fields.kind == EntryKind::Custom || fields.final_name.is_some();

    // Acts that leave the block go back to their home blocks, which mustn't
    // be over: all of them when it becomes a custom entry or a final's block,
    // those of its old category when that changes.
    let timeplan = load(&mut tx).await?;
    if let Some(block) = timeplan.entries.iter().find(|e| e.id == body.id) {
        let leaving = block
            .slots
            .iter()
            .filter_map(Slot::act)
            .filter(|act| drops_slots || (fields.category != block.category && !act.stored));
        timeplan
            .check_way_home(leaving, Some(block.id))
            .map_err(HttpError::ErrorMessages)?;
    }

    let updated = sqlx::query!(
        r#"
        UPDATE timeplan
        SET
            earliest_start_time = $1,
            duration_seconds = $2,
            label = $3,
            category = $4,
            final_name = $5,
            kind = $6,
            einfahrzeit_seconds = $7,
            show_photos = $8,
            internal = $9
        WHERE id = $10
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
        body.id,
    )
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }
    if drops_slots {
        // Its acts go back to their home blocks.
        sqlx::query!("DELETE FROM timeplan_slots WHERE timeplan_id = ?", body.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    Ok(Json(EditTimeplanEntryResponse {}))
}
