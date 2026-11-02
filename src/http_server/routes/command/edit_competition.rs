use std::sync::Arc;

use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    competition::{Competition, upsert_competition},
    config::{CompetitionConfig, is_valid_iban},
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
/// determining day can't change once categories exist. Admin.
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
    if edited.determining_day != current.determining_day {
        let categories = sqlx::query_scalar!(r#"SELECT COUNT(*) as "count!: i64" FROM categories"#)
            .fetch_one(&mut *transaction)
            .await?;
        if categories > 0 {
            return Err(HttpError::ErrorMessages(
                "Der Stichtag lässt sich nicht mehr ändern: Es gibt schon Kategorien, deren \
                 Altersgrenzen an ihm hängen."
                    .to_string(),
            ));
        }
    }
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
    transaction.commit().await?;
    info!("Edited competition {}", competition.id);
    photos.configure(competition.photos.as_ref());

    let warnings = match &competition.payment_iban {
        Some(iban) if !is_valid_iban(iban) => vec![format!(
            "Die IBAN {iban} ist ungültig; die Vereine bekommen keinen Zahlungs-QR-Code."
        )],
        _ => vec![],
    };
    Ok(Json(EditCompetitionResponse { warnings }))
}
