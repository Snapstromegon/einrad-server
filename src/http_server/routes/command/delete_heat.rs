use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    heats,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::{self, JuryUnitKind},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteHeatResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteHeatBody {
    id: Uuid,
}

/// Delete a heat; its categories belong to no heat afterwards.
///
/// Not possible while a final's slots are filled from it.
#[utoipa::path(
    post,
    tags=["command", "heat"],
    path="/delete_heat",
    request_body=DeleteHeatBody,
    responses(
        (status=200, content_type="application/json", body=DeleteHeatResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=409, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn delete_heat(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<DeleteHeatBody>,
) -> Result<Json<DeleteHeatResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;

    let finals = heats::finals_of(&mut tx, body.id).await?;
    if !finals.is_empty() {
        let names: Vec<String> = finals.iter().map(|(name, _)| format!("„{name}“")).collect();
        return Err(HttpError::Conflict(format!(
            "Die Vorlaufklasse füllt Finalplätze in {} {}; bitte sie dort zuerst durch einen anderen ersetzen.",
            if names.len() == 1 {
                "dem Finale"
            } else {
                "den Finals"
            },
            names.join(", ")
        )));
    }
    jury::ensure_unit_unused(&mut tx, JuryUnitKind::Heat, body.id).await?;
    info!("Deleting heat {}", body.id);
    // Its categories leave it (foreign key).
    let result = sqlx::query!("DELETE FROM heats WHERE id = ?", body.id)
        .execute(&mut *tx)
        .await?;
    if result.rows_affected() == 0 {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }

    tx.commit().await?;
    Ok(Json(DeleteHeatResponse {}))
}
