use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    finals,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteFinalResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteFinalBody {
    name: String,
}

/// Delete a final, with its slots and its timeplan block.
///
/// Not possible once the final has started.
#[utoipa::path(
    post,
    tags=["command", "final"],
    path="/delete_final",
    request_body=DeleteFinalBody,
    responses(
        (status=200, content_type="application/json", body=DeleteFinalResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn delete_final(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::PlanCompetition>,
    Json(body): Json<DeleteFinalBody>,
) -> Result<Json<DeleteFinalResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut tx = begin_write(&db).await?;

    if finals::has_started(&mut tx, &body.name).await? {
        return Err(HttpError::ErrorMessages(
            "Das Finale hat schon begonnen und kann nicht mehr gelöscht werden.".to_string(),
        ));
    }
    info!("Deleting final {:?}", body.name);
    sqlx::query!("DELETE FROM timeplan WHERE final_name = ?", body.name)
        .execute(&mut *tx)
        .await?;
    // Its slots go with it (foreign key).
    let result = sqlx::query!("DELETE FROM finals WHERE name = ?", body.name)
        .execute(&mut *tx)
        .await?;
    if result.rows_affected() == 0 {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }

    tx.commit().await?;
    Ok(Json(DeleteFinalResponse {}))
}
