use std::sync::Arc;

use axum::{Extension, Json};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    bookings::paid_by_club,
    competition::{Competition, Features},
    fees::{AmountDue, fees_by_club},
    http_server::{ClientError, HttpError, HttpServerOptions, extractor::auth::Granted},
    jury::JuryObligation,
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
    utils::club_code,
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ClubOverview {
    id: Uuid,
    name: String,
    /// The sum of the club's payments; only with club balances.
    paid: Option<f64>,
    owner_id: Uuid,
    /// Only with the permission to see club contacts.
    contacts: Option<ClubContacts>,
    /// Short code for the transfer reference, e.g. "EV-K7Q3"; stays the
    /// same when the club is renamed.
    code: String,
    /// What the club owes; only with club balances.
    amount_due: Option<AmountDue>,
    /// Why all the club's fees are waived; `null` if they aren't or without
    /// club balances.
    fee_waiver: Option<String>,
    /// The club's jury obligation, while the jury registration is on; only
    /// with club balances or for the jury lead.
    jury: Option<JuryObligation>,
}

/// The people to reach a club through.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ClubContacts {
    /// The club contact (VereinsansprechpartnerIn): whose account registered
    /// the club.
    contact_name: String,
    contact_email: String,
    contact_email_verified: bool,
    /// The club representative (Vereinsverantwortliche/r); the club contact
    /// unless the club named someone else.
    representative_name: String,
    representative_email: String,
}

/// List all clubs; their balances and contacts only with the permission to
/// see them.
#[utoipa::path(
    get,
    tags=["query", "club"],
    path="/list_clubs",
    responses(
        (status=200, content_type="application/json", body=Vec<ClubOverview>),
        (status=401, content_type="application/json", body=ClientError),
        (status=403, content_type="application/json", body=ClientError),
        (status=500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db, http_options))]
pub async fn list_clubs(
    Extension(db): Extension<ReloadableSqlite>,
    Extension(http_options): Extension<Arc<HttpServerOptions>>,
    _: Granted<permit::ViewClubs>,
    balances: Option<Granted<permit::ViewClubBalances>>,
    contacts: Option<Granted<permit::ViewClubContacts>>,
    jury: Option<Granted<permit::ManageJury>>,
) -> Result<Json<Vec<ClubOverview>>, HttpError> {
    let (balances, contacts) = (balances.is_some(), contacts.is_some());
    let jury = balances || jury.is_some();
    let db = db.get().await.clone();
    let rows = sqlx::query!(
        r#"
        SELECT
            clubs.id as "id!: Uuid",
            clubs.name,
            clubs.owner_id as "owner_id!: Uuid",
            clubs.fee_waiver,
            users.name as owner_name,
            users.email as owner_email,
            users.email_verified as owner_email_verified,
            COALESCE(clubs.responsible_name, users.name) as "responsible_name!: String",
            COALESCE(clubs.responsible_email, users.email) as "responsible_email!: String"
        FROM clubs
        JOIN users ON users.id = clubs.owner_id
        ORDER BY clubs.name
        "#
    )
    .fetch_all(&db)
    .await?;
    let competition = Competition::current(&db).await?;
    let paid = paid_by_club(&db).await?;
    let features = Features::of(competition.as_ref(), &http_options.features);
    let mut fees = fees_by_club(&db, competition.as_ref(), features.jury_registration).await?;
    let clubs = rows
        .into_iter()
        .map(|row| {
            let club_fees = fees.remove(&row.id).unwrap_or_default();
            ClubOverview {
                code: club_code(&row.id),
                amount_due: balances.then_some(club_fees.amount_due),
                fee_waiver: row.fee_waiver.filter(|_| balances),
                jury: club_fees.jury.filter(|_| jury),
                id: row.id,
                name: row.name,
                paid: balances.then(|| paid.get(&row.id).copied().unwrap_or(0.0)),
                owner_id: row.owner_id,
                contacts: contacts.then_some(ClubContacts {
                    contact_name: row.owner_name,
                    contact_email: row.owner_email,
                    contact_email_verified: row.owner_email_verified,
                    representative_name: row.responsible_name,
                    representative_email: row.responsible_email,
                }),
            }
        })
        .collect();
    Ok(Json(clubs))
}
