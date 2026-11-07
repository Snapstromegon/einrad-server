use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    groups::ensure_club_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SetClubResponsibleResponse {
    club_id: Uuid,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetClubResponsibleBody {
    club_id: Uuid,
    /// Empty or missing (together with `email`) means the club's owner.
    name: Option<String>,
    email: Option<String>,
}

/// Set the club's Vereinsverantwortliche/r, who files objections at the event.
///
/// Without a name and email the club's owner is responsible. Clubs can change
/// it while registration is open, admins always.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/set_club_responsible",
    request_body=SetClubResponsibleBody,
    responses(
        (status=200, content_type="application/json", body=SetClubResponsibleResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_club_responsible(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(body): Json<SetClubResponsibleBody>,
) -> Result<Json<SetClubResponsibleResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let name = non_empty(body.name);
    let email = non_empty(body.email);
    match (&name, &email) {
        (None, None) => {}
        (Some(_), Some(email)) if is_plausible_email(email) => {}
        (Some(_), Some(_)) => {
            return Err(HttpError::ErrorMessages(
                "Bitte eine gültige E-Mail-Adresse angeben.".to_string(),
            ));
        }
        _ => {
            return Err(HttpError::ErrorMessages(
                "Bitte Name und E-Mail-Adresse der/des Vereinsverantwortlichen angeben."
                    .to_string(),
            ));
        }
    }
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    ensure_club_access(&mut transaction, &auth, body.club_id).await?;
    sqlx::query!(
        "UPDATE clubs SET responsible_name = ?, responsible_email = ? WHERE id = ?",
        name,
        email,
        body.club_id,
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(Json(SetClubResponsibleResponse {
        club_id: body.club_id,
    }))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Something like `name@domain.tld`, without spaces.
fn is_plausible_email(email: &str) -> bool {
    match email.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !domain.contains('@')
                && !email.contains(char::is_whitespace)
        }
        None => false,
    }
}
