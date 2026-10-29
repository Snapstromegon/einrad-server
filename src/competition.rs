use time::{Date, OffsetDateTime};

use crate::{config::CompetitionConfig, system_status::StatusOptions};

time::serde::format_description!(iso_date, Date, "[year]-[month]-[day]");

/// Event specific settings, stored in the `competition` table.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct Competition {
    pub id: String,
    pub name: String,
    pub location: String,
    #[serde(with = "iso_date")]
    pub start_date: Date,
    #[serde(with = "iso_date")]
    pub end_date: Date,
    /// Ages are calculated against this day.
    #[serde(with = "iso_date")]
    pub determining_day: Date,
    #[serde(with = "time::serde::rfc3339")]
    pub registration_start: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub registration_end: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub music_upload_end: OffsetDateTime,
    pub fee_per_start: f64,
    /// Maximum fee per rider; `None` means no cap.
    pub max_fee_per_rider: Option<f64>,
    /// Where the event takes place.
    pub venue: Option<String>,
    /// Contact for clubs, e.g. about overpayments.
    pub contact_email: Option<String>,
    /// Link to the official announcement (Ausschreibung).
    pub announcement_url: Option<String>,
    /// Where the info board's photo QR code leads.
    pub photos_url: Option<String>,
    #[serde(with = "iso_date::option")]
    pub payment_deadline: Option<Date>,
    pub payment_account_holder: Option<String>,
    pub payment_iban: Option<String>,
    pub payment_bic: Option<String>,
    /// Prefix for the transfer reference, followed by the club name.
    pub payment_reference: Option<String>,
    /// The clubs' jury obligation; `None` means there is none.
    pub jury: Option<JuryRules>,
}

/// The jury rules of a competition, from `[competition.jury]`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct JuryRules {
    /// Minimum age of trained judges on the first competition day.
    pub min_age: i64,
    /// Required jury deployments (Einsätze) per started two Küren.
    pub deployments_per_two_acts: i64,
    /// Fee in € per missing deployment.
    pub missing_deployment_fee: f64,
    /// End of the judge registration; the registration's end without it.
    #[serde(with = "time::serde::rfc3339::option")]
    pub registration_end: Option<OffsetDateTime>,
}

impl Competition {
    /// The name mails and pages use for the event: the current competition's
    /// name, or a neutral fallback before one is configured.
    pub async fn event_name(db: impl sqlx::SqliteExecutor<'_>) -> sqlx::Result<String> {
        Ok(Self::current(db).await?.map_or_else(
            || "Einrad-Anmeldung".to_string(),
            |competition| competition.name,
        ))
    }

    /// Loads the current competition: the one with the latest start date.
    pub async fn current(db: impl sqlx::SqliteExecutor<'_>) -> sqlx::Result<Option<Competition>> {
        let row = sqlx::query!(
            r#"
            SELECT
                id as "id!",
                name,
                location,
                start_date as "start_date: Date",
                end_date as "end_date: Date",
                determining_day as "determining_day: Date",
                registration_start as "registration_start: OffsetDateTime",
                registration_end as "registration_end: OffsetDateTime",
                music_upload_end as "music_upload_end: OffsetDateTime",
                fee_per_start,
                max_fee_per_rider,
                venue,
                contact_email,
                announcement_url,
                photos_url,
                payment_deadline as "payment_deadline: Date",
                payment_account_holder,
                payment_iban,
                payment_bic,
                payment_reference,
                jury_min_age,
                jury_deployments_per_two_acts,
                jury_missing_deployment_fee,
                jury_registration_end as "jury_registration_end: OffsetDateTime"
            FROM competition
            ORDER BY start_date DESC
            LIMIT 1
            "#
        )
        .fetch_optional(db)
        .await?;
        Ok(row.map(|row| {
            let jury = match (
                row.jury_min_age,
                row.jury_deployments_per_two_acts,
                row.jury_missing_deployment_fee,
            ) {
                (Some(min_age), Some(deployments_per_two_acts), Some(missing_deployment_fee)) => {
                    Some(JuryRules {
                        min_age,
                        deployments_per_two_acts,
                        missing_deployment_fee,
                        registration_end: row.jury_registration_end,
                    })
                }
                _ => None,
            };
            Competition {
                id: row.id,
                name: row.name,
                location: row.location,
                start_date: row.start_date,
                end_date: row.end_date,
                determining_day: row.determining_day,
                registration_start: row.registration_start,
                registration_end: row.registration_end,
                music_upload_end: row.music_upload_end,
                fee_per_start: row.fee_per_start,
                max_fee_per_rider: row.max_fee_per_rider,
                venue: row.venue,
                contact_email: row.contact_email,
                announcement_url: row.announcement_url,
                photos_url: row.photos_url,
                payment_deadline: row.payment_deadline,
                payment_account_holder: row.payment_account_holder,
                payment_iban: row.payment_iban,
                payment_bic: row.payment_bic,
                payment_reference: row.payment_reference,
                jury,
            }
        }))
    }

    /// End of the judge registration: the jury's own deadline, or the
    /// registration's end.
    pub fn judge_registration_end(&self) -> OffsetDateTime {
        self.jury
            .as_ref()
            .and_then(|jury| jury.registration_end)
            .unwrap_or(self.registration_end)
    }

    pub fn status_options(&self) -> StatusOptions {
        StatusOptions {
            start_register_date: self.registration_start,
            end_register_date: self.registration_end,
            end_music_upload_date: self.music_upload_end,
            end_judge_register_date: self.jury.as_ref().and_then(|jury| jury.registration_end),
        }
    }
}

/// Inserts the competition from the config or updates it if it already exists.
pub async fn upsert_competition(
    db: impl sqlx::SqliteExecutor<'_>,
    config: &CompetitionConfig,
) -> sqlx::Result<()> {
    let jury = config.jury.as_ref();
    sqlx::query!(
        r#"
        INSERT INTO competition (
            id, name, location, start_date, end_date, determining_day,
            registration_start, registration_end, music_upload_end,
            fee_per_start, max_fee_per_rider, venue, contact_email, announcement_url, photos_url,
            payment_deadline, payment_account_holder, payment_iban, payment_bic, payment_reference,
            jury_min_age, jury_deployments_per_two_acts, jury_missing_deployment_fee,
            jury_registration_end
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT (id) DO UPDATE SET
            name = excluded.name,
            location = excluded.location,
            start_date = excluded.start_date,
            end_date = excluded.end_date,
            determining_day = excluded.determining_day,
            registration_start = excluded.registration_start,
            registration_end = excluded.registration_end,
            music_upload_end = excluded.music_upload_end,
            fee_per_start = excluded.fee_per_start,
            max_fee_per_rider = excluded.max_fee_per_rider,
            venue = excluded.venue,
            contact_email = excluded.contact_email,
            announcement_url = excluded.announcement_url,
            photos_url = excluded.photos_url,
            payment_deadline = excluded.payment_deadline,
            payment_account_holder = excluded.payment_account_holder,
            payment_iban = excluded.payment_iban,
            payment_bic = excluded.payment_bic,
            payment_reference = excluded.payment_reference,
            jury_min_age = excluded.jury_min_age,
            jury_deployments_per_two_acts = excluded.jury_deployments_per_two_acts,
            jury_missing_deployment_fee = excluded.jury_missing_deployment_fee,
            jury_registration_end = excluded.jury_registration_end
        "#,
        config.id,
        config.name,
        config.location,
        config.start_date,
        config.end_date,
        config.determining_day,
        config.registration_start,
        config.registration_end,
        config.music_upload_end,
        config.fee_per_start,
        config.max_fee_per_rider,
        config.venue,
        config.contact_email,
        config.announcement_url,
        config.photos_url,
        config.payment.deadline,
        config.payment.account_holder,
        config.payment.iban,
        config.payment.bic,
        config.payment.reference,
        jury.map(|jury| jury.min_age),
        jury.map(|jury| jury.deployments_per_two_acts),
        jury.map(|jury| jury.missing_deployment_fee),
        jury.and_then(|jury| jury.registration_end),
    )
    .execute(db)
    .await?;
    Ok(())
}
