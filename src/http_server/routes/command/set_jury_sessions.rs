use axum::{Extension, Json};
use serde::Deserialize;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    ids::{JuryAssignmentId, JuryUnitId},
    jury::{JuryEnabled, JuryUnitKind},
    jury_sessions::{self, SessionSplit, SplitResult},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    roles::permit,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetJurySessionsBody {
    pub kind: JuryUnitKind,
    /// The heat's or the final's id.
    pub id: JuryUnitId,
    /// The unit's Durchgänge (jury sessions), each with its categories.
    /// Every category of the unit is in exactly one. A listed `id` keeps that
    /// Durchgang's assignments and overrides; without an `id` it is new; the
    /// unit's Durchgänge that are not listed are removed (merged).
    pub sessions: Vec<SessionSplit>,
    /// The assignments of the removed Durchgänge to keep. They move to the
    /// listed Durchgang that receives most of the removed one's categories;
    /// all others are deleted.
    #[serde(default)]
    pub keep_assignment_ids: Vec<JuryAssignmentId>,
}

/// Split a jury unit into Durchgänge (jury sessions) or merge them again,
/// in one step. Jury lead.
#[utoipa::path(
    post,
    tags=["command", "jury"],
    path="/set_jury_sessions",
    request_body=SetJurySessionsBody,
    responses(
        (status=200, content_type="application/json", body=SplitResult),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_jury_sessions(
    _jury: JuryEnabled,
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageJury>,
    Json(body): Json<SetJurySessionsBody>,
) -> Result<Json<SplitResult>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    info!(
        "Setting the sessions of jury unit {:?} {}",
        body.kind, body.id
    );
    let result = jury_sessions::set_sessions(
        &mut transaction,
        body.kind,
        body.id,
        body.sessions,
        body.keep_assignment_ids,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(result))
}
