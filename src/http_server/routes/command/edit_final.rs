use axum::{Extension, Json, http::StatusCode};
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
pub struct EditFinalResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditFinalBody {
    /// The final to edit.
    id: Uuid,
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
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn edit_final(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<EditFinalBody>,
) -> Result<Json<EditFinalResponse>, HttpError> {
    let new_name = finals::normalize_name(&body.new_name).map_err(HttpError::ErrorMessages)?;
    let fields = body.fields.normalize().map_err(HttpError::ErrorMessages)?;
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;

    let heats = sqlx::query_scalar!(
        r#"SELECT heat_id as "heat_id!: Uuid" FROM final_slots WHERE final_id = ? ORDER BY position"#,
        body.id
    )
    .fetch_all(&mut *tx)
    .await?;
    let Some(name) = finals::name_of(&mut tx, body.id).await? else {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    };

    // Without a new code the final keeps its own.
    let new_code = match body.new_code.as_deref() {
        Some(code) if !code.trim().is_empty() => {
            crate::heats::normalize_code(Some(code), &new_name)
        }
        _ => {
            sqlx::query_scalar!(
                r#"SELECT code as "code!" FROM finals WHERE id = ?"#,
                body.id
            )
            .fetch_one(&mut *tx)
            .await?
        }
    };
    finals::check_free(&mut tx, Some(body.id), &new_name, &new_code)
        .await
        .map_err(HttpError::ErrorMessages)?;
    if new_name != name {
        info!("Renaming final {name:?} to {new_name:?}");
    }

    sqlx::query!(
        r#"
        UPDATE finals
        SET
            name = ?,
            code = ?,
            description = ?,
            act_type = ?,
            is_single_male = ?,
            einfahrzeit_seconds = ?,
            act_duration_seconds = ?,
            judge_duration_seconds = ?
        WHERE id = ?
        "#,
        new_name,
        new_code,
        fields.description,
        fields.act_type,
        fields.is_single_male,
        fields.einfahrzeit_seconds,
        fields.act_duration_seconds,
        fields.judge_duration_seconds,
        body.id,
    )
    .execute(&mut *tx)
    .await?;

    finals::check_heats(
        &mut tx,
        body.id,
        fields.act_type,
        fields.is_single_male,
        &fields.slot_groups,
    )
    .await
    .map_err(HttpError::ErrorMessages)?;
    if finals::group_slot_ids(heats) != fields.slot_groups {
        if finals::has_started(&mut tx, body.id).await? {
            return Err(HttpError::ErrorMessages(
                "Das Finale hat schon begonnen, seine Finalplätze bleiben, wie sie sind."
                    .to_string(),
            ));
        }
        info!("Replacing the slots of final {new_name:?}");
        finals::replace_slots(&mut tx, body.id, &fields.slot_groups).await?;
    }

    tx.commit().await?;
    Ok(Json(EditFinalResponse {}))
}
