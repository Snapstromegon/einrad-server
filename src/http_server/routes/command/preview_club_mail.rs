use axum::{Extension, Json};
use tracing::instrument;

use crate::{
    club_mails::{self, Access, ClubMailInput, PreparedMails},
    competition::Features,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

/// Renders a club mail (Rundmail) as `send_club_mail` would send it, without
/// sending or keeping anything. Admin, jury lead and treasurer.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/preview_club_mail",
    request_body=ClubMailInput,
    responses(
        (status=200, content_type="application/json", body=PreparedMails),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, input))]
#[axum::debug_handler]
pub async fn preview_club_mail(
    Extension(db): Extension<ReloadableSqlite>,
    granted: Granted<permit::MailClubs>,
    Json(input): Json<ClubMailInput>,
) -> Result<Json<PreparedMails>, HttpError> {
    let db = db.get().await.clone();
    let features = Features::current(&db).await?;
    let prepared = club_mails::prepare(&db, &input, Access::of(&granted, features)).await?;
    Ok(Json(prepared))
}
