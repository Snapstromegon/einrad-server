use crate::ids::{ClubId, UserId};
use axum::{Extension, Json};
use db_core::{prelude::*, queries::club::ClubListEntry};
use tracing::instrument;

use crate::{
    fees::AmountDue,
    http_server::{ClientError, HttpError, extractor::auth::Granted},
    jury::JuryObligation,
    roles::permit,
    utils::club_code,
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ClubOverview {
    id: ClubId,
    name: String,
    /// The sum of the club's payments; only with club balances.
    paid: Option<f64>,
    owner_id: UserId,
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
#[instrument(skip(repo))]
pub async fn list_clubs(
    Extension(repo): Extension<DbHandle>,
    auth: Granted<permit::ViewClubs>,
    balances: Option<Granted<permit::ViewClubBalances>>,
    contacts: Option<Granted<permit::ViewClubContacts>>,
    jury: Option<Granted<permit::ManageJury>>,
) -> Result<Json<Vec<ClubOverview>>, HttpError> {
    let (balances, contacts) = (balances.is_some(), contacts.is_some());
    let jury = balances || jury.is_some();
    let clubs = repo
        .list_clubs(&auth.actor())
        .await?
        .into_iter()
        .map(|entry| {
            let ClubListEntry {
                club: row,
                paid,
                fees,
            } = entry;
            ClubOverview {
                code: club_code(&row.id),
                amount_due: balances.then_some(fees.amount_due),
                fee_waiver: row.fee_waiver.filter(|_| balances),
                jury: fees.jury.filter(|_| jury),
                id: row.id,
                name: row.name,
                paid: balances.then_some(paid),
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
