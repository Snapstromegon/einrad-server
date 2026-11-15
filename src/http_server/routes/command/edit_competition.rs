use std::sync::Arc;

use axum::{Extension, Json};
use db_core::{commands::competition::EditCompetition, prelude::*};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use super::super::http_types::{Competition, PhotosSettings};
use crate::{
    config::{CompetitionConfig, is_valid_iban},
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    photos::Photos,
    roles::permit,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditCompetitionBody {
    /// The new settings; the id is ignored, it never changes.
    competition: Competition,
    /// Where the info board's photos come from; `None` turns them off.
    photos: Option<PhotosSettings>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EditCompetitionResponse {
    /// Problems that don't stop the save, e.g. an invalid IBAN.
    warnings: Vec<String>,
}

/// `None` for an empty or blank text.
fn filled(text: Option<String>) -> Option<String> {
    text.map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

/// Changes the competition's settings, also while the registration is open
/// (ADR 0005). Validated like the competition file; applies right away. The
/// categories' birthday bounds follow the determining day, as they're derived
/// from it. Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/edit_competition",
    request_body=EditCompetitionBody,
    responses(
        (status=200, content_type="application/json", body=EditCompetitionResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=404, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(repo, photos, body))]
#[axum::debug_handler]
pub async fn edit_competition(
    Extension(repo): Extension<DbHandle>,
    Extension(photos): Extension<Arc<Photos>>,
    auth: Granted<permit::ManageCompetition>,
    Json(body): Json<EditCompetitionBody>,
) -> Result<Json<EditCompetitionResponse>, HttpError> {
    let edited = body.competition;
    let mut warnings = vec![];
    let competition = Competition {
        name: edited.name.trim().to_string(),
        location: edited.location.trim().to_string(),
        venue: filled(edited.venue),
        contact_email: filled(edited.contact_email),
        announcement_url: filled(edited.announcement_url),
        photos_url: filled(edited.photos_url),
        payment_account_holder: filled(edited.payment_account_holder),
        payment_iban: filled(edited.payment_iban),
        payment_bic: filled(edited.payment_bic),
        payment_reference: filled(edited.payment_reference),
        photos: body
            .photos
            .map(|photos| PhotosSettings {
                source: photos.source.trim().to_string(),
                user: filled(photos.user),
                password: photos.password.filter(|password| !password.is_empty()),
            })
            .filter(|photos| !photos.source.is_empty()),
        ..edited
    };
    // The id is ignored: the current competition is edited.
    let competition: db_core::competition::Competition = competition.into();
    let config = CompetitionConfig::from(&competition);
    config.validate().map_err(HttpError::ErrorMessages)?;
    let (photo_settings, iban) = (competition.photos.clone(), competition.payment_iban.clone());
    repo.edit_competition(&auth.actor(), EditCompetition { competition })
        .await?;
    info!("Edited the competition");
    photos.configure(photo_settings.as_ref());

    if let Some(iban) = &iban
        && !is_valid_iban(iban)
    {
        warnings.push(format!(
            "Die IBAN {iban} ist ungültig; die Vereine bekommen keinen Zahlungs-QR-Code."
        ));
    }
    Ok(Json(EditCompetitionResponse { warnings }))
}
