use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::JuryEnabled,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteJuryCategoryResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteJuryCategoryBody {
    id: Uuid,
}

/// Delete a jury category. Refused while judges are registered for it or
/// assigned to it.
/// Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/delete_jury_category",
    request_body=DeleteJuryCategoryBody,
    responses(
        (status=200, content_type="application/json", body=DeleteJuryCategoryResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn delete_jury_category(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<DeleteJuryCategoryBody>,
) -> Result<Json<DeleteJuryCategoryResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    let registrations = sqlx::query_scalar!(
        r#"
        SELECT (
            (SELECT COUNT(*) FROM judge_registration WHERE jury_category_id = ?1)
            + (SELECT COUNT(*) FROM jury_assignments WHERE jury_category_id = ?1)
        ) as "count!: i64"
        "#,
        body.id
    )
    .fetch_one(&mut *transaction)
    .await?;
    if registrations > 0 {
        return Err(HttpError::ErrorMessages(
            "Für diese Jury-Kategorie sind JurorInnen gemeldet oder eingeteilt. Erst das ändern, dann löschen."
                .to_string(),
        ));
    }
    info!("Deleting jury category {}", body.id);
    let result = sqlx::query!("DELETE FROM jury_categories WHERE id = ?", body.id)
        .execute(&mut *transaction)
        .await?;
    if result.rows_affected() == 0 {
        return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
    }
    transaction.commit().await?;
    Ok(Json(DeleteJuryCategoryResponse {}))
}
