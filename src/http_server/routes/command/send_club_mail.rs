use std::sync::Arc;

use axum::{Extension, Json};
use tracing::{info, instrument};

use crate::{
    club_mails::{self, Access, ClubMailInput, SentClubMail},
    competition::Features,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    mailer::Mailer,
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

/// Send a club mail (Rundmail): one mail per club, rendered with its data,
/// to its club contact and club representative, and one per typed-in
/// address. Every mail is rendered before the first one goes out; one that
/// fails doesn't stop the others. Returns the mail as the log keeps it.
/// Admin, jury lead and treasurer.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/send_club_mail",
    request_body=ClubMailInput,
    responses(
        (status=200, content_type="application/json", body=SentClubMail),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, mailer, input))]
#[axum::debug_handler]
pub async fn send_club_mail(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(mailer): Extension<Arc<Mailer>>,
    granted: Granted<permit::MailClubs>,
    Json(input): Json<ClubMailInput>,
) -> Result<Json<SentClubMail>, HttpError> {
    let db = db.get().await.clone();
    let features = Features::current(&db).await?;
    let prepared = club_mails::prepare(&db, &input, Access::of(&granted, features)).await?;
    info!(
        "{} sends a club mail of {} mails",
        granted.name,
        prepared.mails.len()
    );
    let club_mail = club_mails::record(&db, &granted, &input, prepared).await?;
    // On its own task, so all mails go out even if the request goes away.
    let sent = tokio::spawn(club_mails::send(db, mailer, club_mail))
        .await
        .map_err(|_| HttpError::InternalServerError)?;
    Ok(Json(sent))
}
