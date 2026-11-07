use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    competition::Competition,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::JuryEnabled,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetJuryPlanPublishedResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetJuryPlanPublishedBody {
    published: bool,
}

/// Publish the Juryplan to the clubs, or take it back. Each club then sees
/// its own judges' assignments. Publishing waits for the end of the judge
/// registration, so clubs can't change their judges under the plan. Admin
/// only.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/set_jury_plan_published",
    request_body=SetJuryPlanPublishedBody,
    responses(
        (status=200, content_type="application/json", body=SetJuryPlanPublishedResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_jury_plan_published(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<SetJuryPlanPublishedBody>,
) -> Result<Json<SetJuryPlanPublishedResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    let now = time::OffsetDateTime::now_utc();
    if body.published
        && let Some(competition) = Competition::current(&mut *transaction).await?
        && now <= competition.judge_registration_end()
    {
        let end = competition.judge_registration_end();
        return Err(HttpError::ErrorMessages(format!(
            "Der Juryplan kann erst nach dem Ende der Jury-Anmeldung ({:02}.{:02}.{}) veröffentlicht werden.",
            end.day(),
            u8::from(end.month()),
            end.year()
        )));
    }
    let published_at = body.published.then_some(now);
    info!("Juryplan published: {}", body.published);
    sqlx::query!(
        "UPDATE competition SET jury_plan_published_at = ?",
        published_at
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(SetJuryPlanPublishedResponse {}))
}
