use axum::{Extension, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    gender::Gender,
    groups::ensure_club_access,
    http_server::{ClientError, HttpError, extractor::auth::Auth},
    people::{self, Person},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
    rider_registration::{self, PartnerClubs},
    roles::Permission,
    system_status::Capabilities,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct AddRiderResponse {
    rider_id: Uuid,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddRiderBody {
    club_id: Uuid,
    firstname: String,
    lastname: String,
    #[serde(with = "time::serde::iso8601")]
    birthdate: time::OffsetDateTime,
    /// Required, unless it follows from the single (`single_female` or
    /// `single_male`).
    #[serde(default)]
    gender: Option<Gender>,
    single_male: bool,
    single_female: bool,
    pair: bool,
    partner_id: Option<Uuid>,
    partner_name: Option<String>,
}

/// Add a rider to a club.
///
/// Only the club (or an admin) can add riders to it. An explicit
/// `partner_id` must be a rider of the same club; admins can also link
/// partners of other clubs.
#[utoipa::path(
    post,
    tags=["command", "club"],
    path="/add_rider",
    request_body=AddRiderBody,
    responses(
        (status=200, content_type="application/json", body=AddRiderResponse),
        (status=400, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn add_rider(
    Extension(db): Extension<ReloadableSqlite>,
    auth: Auth,
    capabilities: Capabilities,
    Json(mut body): Json<AddRiderBody>,
) -> Result<Json<AddRiderResponse>, HttpError> {
    if !capabilities.can_register_rider {
        return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
    }
    let (single, gender) = single_from_flags(body.single_male, body.single_female, body.gender)?;
    let gender = gender
        .ok_or_else(|| HttpError::ErrorMessages("Bitte gib das Geschlecht an.".to_string()))?;
    people::trim_name(&mut body.firstname);
    people::trim_name(&mut body.lastname);
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    ensure_club_access(&mut transaction, &auth, body.club_id).await?;
    people::ensure_name_is_free(
        &mut transaction,
        Person::Rider,
        body.club_id,
        &body.firstname,
        &body.lastname,
        None,
    )
    .await?;

    let rider_id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO rider (id, club_id, firstname, lastname, birthdate, gender)
        VALUES (?, ?, ?, ?, ?, ?);
        "#,
        rider_id,
        body.club_id,
        body.firstname,
        body.lastname,
        body.birthdate,
        gender,
    )
    .execute(&mut *transaction)
    .await
    .map_err(|e| people::map_duplicate(e, &body.firstname, &body.lastname))?;

    if single {
        rider_registration::set_single(&mut transaction, rider_id, true).await?;
    }
    if body.pair {
        match body.partner_id {
            Some(partner_id) => {
                rider_registration::link_pair(
                    &mut transaction,
                    rider_id,
                    partner_id,
                    partner_clubs(&auth),
                )
                .await?;
            }
            None => {
                rider_registration::request_pair(
                    &mut transaction,
                    rider_id,
                    body.partner_name.as_deref(),
                )
                .await?;
            }
        }
    }

    transaction.commit().await?;
    Ok(Json(AddRiderResponse { rider_id }))
}

/// Whose riders the user can pair a rider with: clubs their own, the
/// organizer any.
pub(super) fn partner_clubs(auth: &Auth) -> PartnerClubs {
    if auth.can(Permission::ManageRegistrations) {
        PartnerClubs::Any
    } else {
        PartnerClubs::Same
    }
}

/// The single from the API's `single_male` / `single_female` flags and the
/// rider's gender. A single is Einzel weiblich or männlich by the gender, so
/// the flag must match it; without a gender it gives one.
pub(super) fn single_from_flags(
    single_male: bool,
    single_female: bool,
    gender: Option<Gender>,
) -> Result<(bool, Option<Gender>), HttpError> {
    let single_gender = match (single_male, single_female) {
        (true, true) => {
            return Err(HttpError::ErrorMessages(
                "Ein Fahrer kann nur in einer Einzelkür starten (Einzel M oder Einzel W)."
                    .to_string(),
            ));
        }
        (true, false) => Some(Gender::Male),
        (false, true) => Some(Gender::Female),
        (false, false) => None,
    };
    match (gender, single_gender) {
        (Some(gender), Some(single)) if gender != single => {
            Err(HttpError::ErrorMessages(match gender {
                Gender::Female => "Eine Fahrerin startet in Einzel weiblich.".to_string(),
                Gender::Male => "Ein Fahrer startet in Einzel männlich.".to_string(),
            }))
        }
        _ => Ok((single_gender.is_some(), gender.or(single_gender))),
    }
}
