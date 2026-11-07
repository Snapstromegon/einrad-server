use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    competition::Competition,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::{self, JuryCategoryFields, JuryEnabled},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditJuryCategoryResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditJuryCategoryBody {
    id: Uuid,
    #[serde(flatten)]
    fields: JuryCategoryFields,
}

/// Change a jury category. Changing its deployments changes every club's
/// registered deployments. Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/edit_jury_category",
    request_body=EditJuryCategoryBody,
    responses(
        (status=200, content_type="application/json", body=EditJuryCategoryResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn edit_jury_category(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<EditJuryCategoryBody>,
) -> Result<Json<EditJuryCategoryResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    let competition = Competition::current(&mut *transaction).await?;
    let fields = body
        .fields
        .normalize(competition.as_ref())
        .map_err(HttpError::ErrorMessages)?;
    info!("Editing jury category {:?}", fields.name);
    let result = sqlx::query!(
        r#"
        UPDATE jury_categories SET
            name = ?, deployments = ?, day = ?,
            needed_technical = ?, needed_performance = ?, needed_dismounts = ?
        WHERE id = ?
        "#,
        fields.name,
        fields.deployments,
        fields.day,
        fields.needed_technical,
        fields.needed_performance,
        fields.needed_dismounts,
        body.id,
    )
    .execute(&mut *transaction)
    .await;
    match result {
        Ok(result) if result.rows_affected() == 0 => {
            return Err(HttpError::StatusCode(StatusCode::NOT_FOUND));
        }
        Ok(_) => {}
        Err(e) if jury::is_unique_violation(&e) => return Err(jury::duplicate_name(&fields.name)),
        Err(e) => return Err(e.into()),
    }
    transaction.commit().await?;
    Ok(Json(EditJuryCategoryResponse {}))
}
