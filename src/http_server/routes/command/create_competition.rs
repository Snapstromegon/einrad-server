use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use time::{Date, Month, OffsetDateTime};
use tracing::instrument;
use utoipa::ToSchema;

use crate::{
    competition::Features,
    config::{Branding, CompetitionConfig, CompetitionFile, PaymentConfig},
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
    seed,
};

/// The longest id; it ends up in file names and URLs.
const MAX_ID_LEN: usize = 64;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateCompetitionBody {
    /// Lowercase letters, digits and single hyphens, e.g. `testcup-2027`;
    /// never changes afterwards.
    id: String,
    name: String,
    location: String,
    #[serde(with = "crate::competition::iso_date")]
    #[schema(value_type = String, example = "2027-06-05")]
    start_date: Date,
    #[serde(with = "crate::competition::iso_date")]
    #[schema(value_type = String, example = "2027-06-06")]
    end_date: Date,
    #[serde(with = "time::serde::rfc3339")]
    registration_start: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    registration_end: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    music_upload_end: OffsetDateTime,
    fee_per_start: f64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateCompetitionResponse {
    pub id: String,
}

/// Every problem of the form, as German sentences; empty if it's fine.
fn problems(body: &CreateCompetitionBody) -> Vec<String> {
    let mut problems = vec![];
    let id = body.id.as_str();
    let id_chars = id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if id.is_empty()
        || !id_chars
        || id.len() > MAX_ID_LEN
        || id.starts_with('-')
        || id.ends_with('-')
        || id.contains("--")
    {
        problems.push(format!(
            "Die ID darf nur aus Kleinbuchstaben, Ziffern und einzelnen Bindestrichen \
             bestehen, nicht mit einem Bindestrich beginnen oder enden und höchstens \
             {MAX_ID_LEN} Zeichen lang sein."
        ));
    }
    if body.name.trim().is_empty() {
        problems.push("Der Name darf nicht leer sein.".to_string());
    }
    if body.location.trim().is_empty() {
        problems.push("Der Ort darf nicht leer sein.".to_string());
    }
    if body.end_date < body.start_date {
        problems.push("Das Ende darf nicht vor dem Beginn liegen.".to_string());
    }
    if !body.fee_per_start.is_finite() || body.fee_per_start < 0.0 {
        problems.push("Die Gebühr pro Start darf nicht negativ sein.".to_string());
    }
    let start = body.start_date.midnight().assume_utc();
    if body.registration_start >= body.registration_end {
        problems.push("Der Anmeldebeginn muss vor dem Anmeldeschluss liegen.".to_string());
    }
    if body.registration_end > start {
        problems.push("Die Anmeldung muss vor dem Beginn des Wettkampfs enden.".to_string());
    }
    if body.music_upload_end > start {
        problems.push("Der Musik-Upload muss vor dem Beginn des Wettkampfs enden.".to_string());
    }
    problems
}

/// Creates the competition from scratch in setup mode (ADR 0005, ADR 0011),
/// as an alternative to importing a file: only what the competition needs to
/// exist. The determining day is 1 June of the start year; categories,
/// timeplan, branding and the rest are added in the competition editor.
/// Refused while a competition exists (ADR 0001). Admin.
#[utoipa::path(
    post,
    tags=["command", "competition"],
    path="/create_competition",
    request_body=CreateCompetitionBody,
    responses(
        (status=200, content_type="application/json", body=CreateCompetitionResponse),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, body))]
#[axum::debug_handler]
pub async fn create_competition(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageCompetition>,
    Json(body): Json<CreateCompetitionBody>,
) -> Result<Json<CreateCompetitionResponse>, HttpError> {
    let problems = problems(&body);
    if !problems.is_empty() {
        return Err(HttpError::ErrorMessages(problems.join(" ")));
    }
    let determining_day = Date::from_calendar_date(body.start_date.year(), Month::June, 1)
        .map_err(|_| HttpError::InternalServerError)?;
    let id = body.id;
    let file = CompetitionFile {
        competition: CompetitionConfig {
            id: id.clone(),
            name: body.name.trim().to_string(),
            location: body.location.trim().to_string(),
            start_date: body.start_date,
            end_date: body.end_date,
            determining_day,
            registration_start: body.registration_start,
            registration_end: body.registration_end,
            music_upload_end: body.music_upload_end,
            fee_per_start: body.fee_per_start,
            max_fee_per_rider: None,
            venue: None,
            contact_email: None,
            announcement_url: None,
            photos_url: None,
            payment: PaymentConfig::default(),
            jury: None,
            features: Features::default(),
            photos: None,
            logo_svg: Branding::Keep,
            logo_url: Branding::Keep,
            break_image_svg: Branding::Keep,
            break_image_url: Branding::Keep,
            sponsors: None,
            primary_color: Branding::Keep,
            secondary_color: Branding::Keep,
        },
        categories: vec![],
        finals: vec![],
        timeplan: vec![],
        jury_categories: vec![],
    };
    let db = db.get().await.clone();
    seed::create(&db, &file)
        .await
        .map_err(HttpError::ErrorMessages)?;
    Ok(Json(CreateCompetitionResponse { id }))
}
