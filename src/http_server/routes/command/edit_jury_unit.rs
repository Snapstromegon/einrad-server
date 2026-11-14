use axum::{Extension, Json, http::StatusCode};
use serde::Serialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::{JuryEnabled, JuryUnitFields, JuryUnitKind},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditJuryUnitResponse {}

/// Change the jury settings of a heat's Vorlauf or of a final: whether judges
/// can register for it, its deployments and the judges it needs per task.
/// Changing its deployments changes every club's registered deployments.
/// Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/edit_jury_unit",
    request_body=JuryUnitFields,
    responses(
        (status=200, content_type="application/json", body=EditJuryUnitResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn edit_jury_unit(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<JuryUnitFields>,
) -> Result<Json<EditJuryUnitResponse>, HttpError> {
    body.validate().map_err(HttpError::ErrorMessages)?;
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    info!("Editing jury unit {:?} {}", body.kind, body.id);
    let result = match body.kind {
        JuryUnitKind::Heat => {
            sqlx::query!(
                r#"
                UPDATE heats SET
                    jury_registrable = $1, jury_deployments = $2,
                    jury_needed_technical = $3, jury_needed_performance = $4,
                    jury_needed_dismounts = $5
                WHERE id = $6
                "#,
                body.registrable,
                body.deployments,
                body.needed_technical,
                body.needed_performance,
                body.needed_dismounts,
                body.id,
            )
            .execute(&mut *transaction)
            .await?
        }
        JuryUnitKind::Final => {
            sqlx::query!(
                r#"
                UPDATE finals SET
                    jury_registrable = $1, jury_deployments = $2,
                    jury_needed_technical = $3, jury_needed_performance = $4,
                    jury_needed_dismounts = $5
                WHERE id = $6
                "#,
                body.registrable,
                body.deployments,
                body.needed_technical,
                body.needed_performance,
                body.needed_dismounts,
                body.id,
            )
            .execute(&mut *transaction)
            .await?
        }
    };
    if result.rows_affected() == 0 {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }
    transaction.commit().await?;
    Ok(Json(EditJuryUnitResponse {}))
}
