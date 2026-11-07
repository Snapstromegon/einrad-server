use axum::{Extension, Json, http::StatusCode};
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
pub struct EditFinalResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditFinalBody {
    /// The final to edit.
    name: String,
    new_name: String,
    #[serde(flatten)]
    fields: FinalFields,
}

/// Edit a final.
///
/// Every field is overwritten. Its slots are only created anew when the slot
/// groups change, which isn't possible once the final has started; a slot
/// with the same group label and number keeps its placed act. Placed acts
/// stay when their category is taken off the final.
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

    let labels = sqlx::query_scalar!(
        "SELECT group_label FROM final_slots WHERE final_name = ? ORDER BY position",
        body.name
    )
    .fetch_all(&mut *tx)
    .await?;
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM finals WHERE name = ?) as "exists!: bool""#,
        body.name
    )
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }

    if new_name != body.name {
        let taken = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM finals WHERE name = ?) as "taken!: bool""#,
            new_name
        )
        .fetch_one(&mut *tx)
        .await?;
        if taken {
            return Err(HttpError::ErrorMessages(format!(
                "Ein Finale „{new_name}“ gibt es schon."
            )));
        }
        info!("Renaming final {:?} to {new_name:?}", body.name);
        sqlx::query!(
            "UPDATE timeplan SET final_name = ? WHERE final_name = ?",
            new_name,
            body.name
        )
        .execute(&mut *tx)
        .await?;
    }

    // The slots follow a rename by their foreign key.
    sqlx::query!(
        r#"
        UPDATE finals
        SET
            name = ?,
            description = ?,
            act_type = ?,
            is_single_male = ?,
            einfahrzeit_seconds = ?,
            act_duration_seconds = ?,
            judge_duration_seconds = ?
        WHERE name = ?
        "#,
        new_name,
        fields.description,
        fields.act_type,
        fields.is_single_male,
        fields.einfahrzeit_seconds,
        fields.act_duration_seconds,
        fields.judge_duration_seconds,
        body.name,
    )
    .execute(&mut *tx)
    .await?;

    if finals::group_slots(labels.iter().map(String::as_str)) != fields.slot_groups {
        if finals::has_started(&mut tx, &new_name).await? {
            return Err(HttpError::ErrorMessages(
                "Das Finale hat schon begonnen, seine Finalplätze bleiben, wie sie sind."
                    .to_string(),
            ));
        }
        info!("Replacing the slots of final {new_name:?}");
        finals::replace_slots(&mut tx, &new_name, &fields.slot_groups).await?;
    }

    finals::check_categories(&mut tx, &fields.categories)
        .await
        .map_err(HttpError::ErrorMessages)?;
    finals::set_categories(&mut tx, &new_name, &fields.categories).await?;

    tx.commit().await?;
    Ok(Json(EditFinalResponse {}))
}
