use crate::ids::RiderId;
use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    gender::Gender,
    groups::ensure_rider_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    people::{self, Person},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditRiderResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditRiderBody {
    rider_id: RiderId,
    firstname: String,
    lastname: String,
    #[serde(with = "time::serde::iso8601")]
    birthdate: time::OffsetDateTime,
    /// Missing keeps the stored gender. A single follows it to Einzel
    /// weiblich or männlich.
    #[serde(default)]
    gender: Option<Gender>,
}

/// Edit a rider's own data; their starts are changed with the start
/// commands.
///
/// Only the rider's club (or an admin) can edit them.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/edit_rider",
    request_body=EditRiderBody,
    responses(
        (status=200, content_type="application/json", body=EditRiderResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn edit_rider(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(mut body): Json<EditRiderBody>,
) -> Result<Json<EditRiderResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    people::trim_name(&mut body.firstname);
    people::trim_name(&mut body.lastname);
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    let club_id = ensure_rider_access(&mut transaction, &auth, body.rider_id).await?;
    people::ensure_name_is_free(
        &mut transaction,
        Person::Rider,
        club_id,
        &body.firstname,
        &body.lastname,
        Some(body.rider_id.0),
    )
    .await?;

    sqlx::query!(
        r#"
        UPDATE rider SET firstname = $1, lastname = $2, birthdate = $3, gender = COALESCE($4, gender)
        WHERE id = $5
        "#,
        body.firstname,
        body.lastname,
        body.birthdate,
        body.gender,
        body.rider_id,
    )
    .execute(&mut *transaction)
    .await
    .map_err(|e| people::map_duplicate(e, &body.firstname, &body.lastname))?;

    transaction.commit().await?;
    Ok(Json(EditRiderResponse {}))
}
