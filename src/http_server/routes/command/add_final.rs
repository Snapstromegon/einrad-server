use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

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
    /// The readable name, unique, e.g. "Finale Einzel männlich Expert".
    name: String,
    /// The short code, unique, for tight spaces and references in files,
    /// e.g. "E EM". Without: the name.
    #[serde(default)]
    code: Option<String>,
    #[serde(flatten)]
    fields: FinalFields,
}

/// Add a final: reserved slots for the best riders of the heats.
///
/// It isn't planned yet; add a timeplan entry with its `final_id` for that.
#[utoipa::path(
    post,
    tags=["command", "final"],
    path="/add_final",
    request_body=AddFinalBody,
    responses(
        (status=200, content_type="application/json", body=AddFinalResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=409, content_type="application/json", body=ClientError),
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
    let code = crate::heats::normalize_code(body.code.as_deref(), &name);
    let fields = body.fields.normalize().map_err(HttpError::ErrorMessages)?;
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;

    finals::check_free(&mut tx, None, &name, &code)
        .await
        .map_err(HttpError::Conflict)?;

    info!("Adding final {name:?}");
    let id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO finals (
            id, name, code, description, act_type, is_single_male, "order",
            einfahrzeit_seconds, act_duration_seconds, judge_duration_seconds
        )
        VALUES (?, ?, ?, ?, ?, ?, (SELECT COALESCE(MAX("order"), 0) + 1 FROM finals), ?, ?, ?)
        "#,
        id,
        name,
        code,
        fields.description,
        fields.act_type,
        fields.is_single_male,
        fields.einfahrzeit_seconds,
        fields.act_duration_seconds,
        fields.judge_duration_seconds,
    )
    .execute(&mut *tx)
    .await?;
    finals::check_heats(
        &mut tx,
        id,
        fields.act_type,
        fields.is_single_male,
        &fields.slot_groups,
    )
    .await
    .map_err(HttpError::ErrorMessages)?;
    finals::replace_slots(&mut tx, id, &fields.slot_groups).await?;

    tx.commit().await?;
    Ok(Json(AddFinalResponse {}))
}
