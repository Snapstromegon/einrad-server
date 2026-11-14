use crate::ids::{JudgeId, JuryUnitId};
use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::{self, JuryEnabled, JuryUnitKind},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct ResolveUnmappedRegistrationResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ResolveUnmappedRegistrationBody {
    /// The id of the unmapped registration.
    id: i64,
    /// The jury unit to register the judge for instead; `None` drops the
    /// registration.
    unit: Option<UnitRef>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UnitRef {
    kind: JuryUnitKind,
    id: JuryUnitId,
}

/// Resolve a registration the migration to jury units could not map (JURY-5):
/// register the judge for a heat's Vorlauf or a final, or drop it. Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/resolve_unmapped_registration",
    request_body=ResolveUnmappedRegistrationBody,
    responses(
        (status=200, content_type="application/json", body=ResolveUnmappedRegistrationResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn resolve_unmapped_registration(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<ResolveUnmappedRegistrationBody>,
) -> Result<Json<ResolveUnmappedRegistrationResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    let unmapped = sqlx::query!(
        r#"
        SELECT judge_id as "judge_id!: JudgeId", task as "task!: jury::JudgeTask"
        FROM judge_registration_unmapped WHERE id = ?
        "#,
        body.id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(HttpError::StatusCode(StatusCode::NOT_FOUND))?;
    info!("Resolving unmapped registration {}", body.id);
    if let Some(unit) = body.unit {
        match jury::unit_is_registrable(&mut transaction, unit.kind, unit.id).await? {
            None => return Err(HttpError::StatusCode(StatusCode::NOT_FOUND)),
            Some(false) => return Err(jury::not_registrable()),
            Some(true) => {}
        }
        let (heat_id, final_id) = jury::unit_columns(unit.kind, unit.id);
        sqlx::query!(
            "INSERT OR IGNORE INTO judge_registration (judge_id, heat_id, final_id, task) VALUES (?, ?, ?, ?)",
            unmapped.judge_id,
            heat_id,
            final_id,
            unmapped.task,
        )
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query!(
        "DELETE FROM judge_registration_unmapped WHERE id = ?",
        body.id
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ResolveUnmappedRegistrationResponse {}))
}
