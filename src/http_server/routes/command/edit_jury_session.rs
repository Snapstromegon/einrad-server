use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    ids::JurySessionId,
    jury::JuryEnabled,
    jury_sessions,
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct EditJurySessionResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditJurySessionBody {
    pub id: JurySessionId,
    /// Judges this Durchgang needs for Technik; `null` or absent: the unit's.
    #[serde(default)]
    pub needed_technical: Option<i64>,
    #[serde(default)]
    pub needed_performance: Option<i64>,
    #[serde(default)]
    pub needed_dismounts: Option<i64>,
}

/// Override the judges a Durchgang (jury session) needs per task; `null` or
/// absent follows the jury unit's counts. Lowering a count below an occupied
/// seat is refused (409). Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/edit_jury_session",
    request_body=EditJurySessionBody,
    responses(
        (status=200, content_type="application/json", body=EditJurySessionResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=409, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn edit_jury_session(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<EditJurySessionBody>,
) -> Result<Json<EditJurySessionResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    info!("Editing jury session {}", body.id);
    let unit = jury_sessions::unit_of_session(&mut transaction, body.id).await?;
    let needed_before = jury_sessions::needed_snapshot(&mut transaction, unit).await?;
    jury_sessions::set_session_needed(
        &mut transaction,
        body.id,
        body.needed_technical,
        body.needed_performance,
        body.needed_dismounts,
    )
    .await?;
    jury_sessions::ensure_seats_fit(&mut transaction, needed_before).await?;
    transaction.commit().await?;
    Ok(Json(EditJurySessionResponse {}))
}
