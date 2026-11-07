use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    finals::{self, FinalFields},
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct AddFinalResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddFinalBody {
    /// Unique, e.g. "Finale 1 Einzel männlich".
    name: String,
    #[serde(flatten)]
    fields: FinalFields,
}

/// Add a final: reserved slots for the best riders of the heats.
///
/// It isn't planned yet; add a timeplan entry with its `final_name` for that.
#[utoipa::path(
    post,
    tags=["command", "final"],
    path="/add_final",
    request_body=AddFinalBody,
    responses(
        (status=200, content_type="application/json", body=AddFinalResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn add_final(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<AddFinalBody>,
) -> Result<Json<AddFinalResponse>, HttpError> {
    let name = finals::normalize_name(&body.name).map_err(HttpError::ErrorMessages)?;
    let fields = body.fields.normalize().map_err(HttpError::ErrorMessages)?;
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;

    let taken = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM finals WHERE name = ?) as "taken!: bool""#,
        name
    )
    .fetch_one(&mut *tx)
    .await?;
    if taken {
        return Err(HttpError::ErrorMessages(format!(
            "Ein Finale „{name}“ gibt es schon."
        )));
    }

    info!("Adding final {name:?}");
    sqlx::query!(
        r#"
        INSERT INTO finals (
            name, description, act_type, is_single_male, "order",
            einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds
        )
        VALUES (?, ?, ?, ?, (SELECT COALESCE(MAX("order"), 0) + 1 FROM finals), ?, ?, ?)
        "#,
        name,
        fields.description,
        fields.act_type,
        fields.is_single_male,
        fields.einfahrzeit_seconds,
        fields.act_duration_seconds,
        fields.judge_duration_seconds,
    )
    .execute(&mut *tx)
    .await?;
    finals::replace_slots(&mut tx, &name, &fields.slot_groups).await?;
    finals::check_categories(&mut tx, &fields.categories)
        .await
        .map_err(HttpError::ErrorMessages)?;
    finals::set_categories(&mut tx, &name, &fields.categories).await?;

    tx.commit().await?;
    Ok(Json(AddFinalResponse {}))
}
