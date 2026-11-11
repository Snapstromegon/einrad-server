use std::sync::Arc;

use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    colors,
    competition::{Competition, upsert_competition},
    config::{Branding, CompetitionConfig, is_valid_iban},
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    photos::{Photos, PhotosSettings},
    reloadable_sqlite::{ReloadableSqlite, begin_write},
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
#[instrument(skip(db, photos, body))]
#[axum::debug_handler]
pub async fn edit_competition(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(photos): Extension<Arc<Photos>>,
    _: Granted<permit::ManageCompetition>,
    Json(body): Json<EditCompetitionBody>,
) -> Result<Json<EditCompetitionResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    let current = Competition::current(&mut *transaction)
        .await?
        .ok_or(HttpError::NotFound)?;
    let edited = body.competition;
    let mut warnings = vec![];
    let competition = Competition {
        id: current.id,
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
    let config = CompetitionConfig::from(&competition);
    config.validate().map_err(HttpError::ErrorMessages)?;
    upsert_competition(&mut *transaction, &config).await?;
    // The form sets both colors; `None` is the default (ADR 0009).
    colors::write(
        &mut *transaction,
        &competition.id,
        &Branding::set_or_reset(competition.primary_color.clone()),
        &Branding::set_or_reset(competition.secondary_color.clone()),
    )
    .await?;
    transaction.commit().await?;
    info!("Edited competition {}", competition.id);
    photos.configure(competition.photos.as_ref());

    if let Some(iban) = &competition.payment_iban
        && !is_valid_iban(iban)
    {
        warnings.push(format!(
            "Die IBAN {iban} ist ungültig; die Vereine bekommen keinen Zahlungs-QR-Code."
        ));
    }
    Ok(Json(EditCompetitionResponse { warnings }))
}
