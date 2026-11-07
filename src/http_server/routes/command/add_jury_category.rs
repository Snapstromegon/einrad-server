use axum::{Extension, Json};
use serde::Serialize;
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
pub struct AddJuryCategoryResponse {
    id: Uuid,
}

/// Add a jury category at the end of the order. Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/add_jury_category",
    request_body=JuryCategoryFields,
    responses(
        (status=200, content_type="application/json", body=AddJuryCategoryResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn add_jury_category(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<JuryCategoryFields>,
) -> Result<Json<AddJuryCategoryResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    let competition = Competition::current(&mut *transaction).await?;
    let fields = body
        .normalize(competition.as_ref())
        .map_err(HttpError::ErrorMessages)?;
    info!("Adding jury category {:?}", fields.name);
    let id = jury::insert_category(&mut transaction, &fields).await?;
    transaction.commit().await?;
    Ok(Json(AddJuryCategoryResponse { id }))
}
