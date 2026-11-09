use std::sync::Arc;

use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::{
    colors,
    competition::{Competition, move_birthday_bound, parse_birthday_bound, upsert_competition},
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

/// Moves the birthday bounds of every category that looked like it was
/// computed from the `old` determining day to the `new` one. Bounds that
/// don't match, or that can't be read, stay.
async fn move_category_bounds(
    transaction: &mut sqlx::SqliteConnection,
    old: time::Date,
    new: time::Date,
) -> Result<(), HttpError> {
    let categories = sqlx::query!(
        r#"SELECT name, from_birthday as "from_birthday: String", to_birthday as "to_birthday: String" FROM categories"#
    )
        .fetch_all(&mut *transaction)
        .await?;
    for category in categories {
        let parse = |bound: &Option<String>| bound.as_deref().and_then(parse_birthday_bound);
        let (from, to) = (parse(&category.from_birthday), parse(&category.to_birthday));
        let moved_from = from.and_then(|bound| move_birthday_bound(bound, old, new));
        let moved_to = to.and_then(|bound| move_birthday_bound(bound, old, new));
        if moved_from.is_none() && moved_to.is_none() {
            continue;
        }
        // Only a moved bound is rewritten; the other keeps its stored text.
        sqlx::query(
            "UPDATE categories SET from_birthday = COALESCE(?, from_birthday), \
             to_birthday = COALESCE(?, to_birthday) WHERE name = ?",
        )
        .bind(moved_from)
        .bind(moved_to)
        .bind(&category.name)
        .execute(&mut *transaction)
        .await?;
    }
    Ok(())
}

/// Changes the competition's settings, also while the registration is open
/// (ADR 0005). Validated like the competition file; applies right away. The
/// determining day moves the category bounds that looked like it along. Admin.
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
        move_category_bounds(
            &mut transaction,
            current.determining_day,
            edited.determining_day,
        )
        .await?;
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

    let warnings = match &competition.payment_iban {
        Some(iban) if !is_valid_iban(iban) => vec![format!(
            "Die IBAN {iban} ist ungültig; die Vereine bekommen keinen Zahlungs-QR-Code."
        )],
        _ => vec![],
    };
    Ok(Json(EditCompetitionResponse { warnings }))
}
