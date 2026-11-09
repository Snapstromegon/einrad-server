use time::{Date, OffsetDateTime};

use crate::{
    colors::HexColor,
    config::{Branding, CompetitionConfig, JuryConfig, PaymentConfig},
    photos::PhotosSettings,
    system_status::StatusOptions,
};

time::serde::format_description!(iso_date, Date, "[year]-[month]-[day]");

/// Event specific settings, stored in the `competition` table.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
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
    /// The features this competition turns on or off; see
    /// [`Features::resolve`].
    pub features: Features,
    /// Where the info board's photos come from; `None` means no photos.
    /// Never sent to clients: it holds a password.
    #[serde(skip)]
    pub photos: Option<PhotosSettings>,
    /// The primary color of every page (see `crate::colors`); `None` for
    /// the default.
    #[schema(value_type = Option<String>, example = "#002d56")]
    pub primary_color: Option<HexColor>,
    /// The secondary color; `None` for the default.
    #[schema(value_type = Option<String>, example = "#009036")]
    pub secondary_color: Option<HexColor>,
}

/// Features that are still being refined, as a competition (or the system
/// config's `[features]`) sets them: on, off, or unset.
#[derive(
    Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct Features {
    /// Clubs register judges, admins plan the jury (see `crate::jury`).
    pub jury_registration: Option<bool>,
    /// Clubs can add an extra description for the moderation while registering
    /// their acts.
    pub moderation_input: Option<bool>,
}

/// Whether each feature is on, after [`Features::resolve`].
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct ResolvedFeatures {
    pub jury_registration: bool,
    pub moderation_input: bool,
}

impl Features {
    /// Whether every feature is unset.
    pub fn is_unset(&self) -> bool {
        *self == Self::default()
    }

    /// Fills in what's unset from `defaults` (the system config's
    /// `[features]`), then each feature's default.
    pub fn resolve(&self, defaults: &Features) -> ResolvedFeatures {
        ResolvedFeatures {
            jury_registration: self
                .jury_registration
                .or(defaults.jury_registration)
                .unwrap_or(cfg!(debug_assertions)),
            moderation_input: self
                .moderation_input
                .or(defaults.moderation_input)
                .unwrap_or(true),
        }
    }

    /// The features of `competition`, or only the defaults without one.
    pub fn of(competition: Option<&Competition>, defaults: &Features) -> ResolvedFeatures {
        competition
            .map(|competition| competition.features.clone())
            .unwrap_or_default()
            .resolve(defaults)
    }

    /// The features of the current competition. Read per request, so a
    /// change applies without a restart.
    pub async fn current(
        db: impl sqlx::SqliteExecutor<'_>,
        defaults: &Features,
    ) -> sqlx::Result<ResolvedFeatures> {
        Ok(Self::of(Competition::current(db).await?.as_ref(), defaults))
    }
}

/// The jury rules of a competition, from `[competition.jury]`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
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
    /// Whether an admin published the current competition's start list and
    /// timeplan. Without a competition nothing is published.
    pub async fn startlist_published(db: impl sqlx::SqliteExecutor<'_>) -> sqlx::Result<bool> {
        Ok(sqlx::query_scalar!(
            r#"
            SELECT startlist_published_at IS NOT NULL as "published!: bool"
            FROM competition ORDER BY start_date DESC LIMIT 1
            "#
        )
        .fetch_optional(db)
        .await?
        .unwrap_or(false))
    }

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
                jury_registration_end as "jury_registration_end: OffsetDateTime",
                feature_jury_registration as "feature_jury_registration: bool",
                feature_moderation_input as "feature_moderation_input: bool",
                photos_source,
                photos_user,
                photos_password,
                primary_color,
                secondary_color
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
                features: Features {
                    jury_registration: row.feature_jury_registration,
                    moderation_input: row.feature_moderation_input,
                },
                photos: row.photos_source.map(|source| PhotosSettings {
                    source,
                    user: row.photos_user,
                    password: row.photos_password,
                }),
                // Only valid colors are written (see `crate::colors::write`).
                primary_color: row.primary_color.and_then(|color| color.parse().ok()),
                secondary_color: row.secondary_color.and_then(|color| color.parse().ok()),
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

impl From<&CompetitionConfig> for Competition {
    /// The competition as the database stores it.
    fn from(config: &CompetitionConfig) -> Self {
        Competition {
            id: config.id.clone(),
            name: config.name.clone(),
            location: config.location.clone(),
            start_date: config.start_date,
            end_date: config.end_date,
            determining_day: config.determining_day,
            registration_start: config.registration_start,
            registration_end: config.registration_end,
            music_upload_end: config.music_upload_end,
            fee_per_start: config.fee_per_start,
            max_fee_per_rider: config.max_fee_per_rider,
            venue: config.venue.clone(),
            contact_email: config.contact_email.clone(),
            announcement_url: config.announcement_url.clone(),
            photos_url: config.photos_url.clone(),
            payment_deadline: config.payment.deadline,
            payment_account_holder: config.payment.account_holder.clone(),
            payment_iban: config.payment.iban.clone(),
            payment_bic: config.payment.bic.clone(),
            payment_reference: config.payment.reference.clone(),
            jury: config.jury.as_ref().map(|jury| JuryRules {
                min_age: jury.min_age,
                deployments_per_two_acts: jury.deployments_per_two_acts,
                missing_deployment_fee: jury.missing_deployment_fee,
                registration_end: jury.registration_end,
            }),
            features: config.features.clone(),
            photos: config.photos.clone(),
            primary_color: config.primary_color.value().cloned(),
            secondary_color: config.secondary_color.value().cloned(),
        }
    }
}

impl From<&Competition> for CompetitionConfig {
    /// The competition as the competition file has it, e.g. to validate an
    /// edit like the file.
    fn from(competition: &Competition) -> Self {
        CompetitionConfig {
            id: competition.id.clone(),
            name: competition.name.clone(),
            location: competition.location.clone(),
            start_date: competition.start_date,
            end_date: competition.end_date,
            determining_day: competition.determining_day,
            registration_start: competition.registration_start,
            registration_end: competition.registration_end,
            music_upload_end: competition.music_upload_end,
            fee_per_start: competition.fee_per_start,
            max_fee_per_rider: competition.max_fee_per_rider,
            venue: competition.venue.clone(),
            contact_email: competition.contact_email.clone(),
            announcement_url: competition.announcement_url.clone(),
            photos_url: competition.photos_url.clone(),
            payment: PaymentConfig {
                deadline: competition.payment_deadline,
                account_holder: competition.payment_account_holder.clone(),
                iban: competition.payment_iban.clone(),
                bic: competition.payment_bic.clone(),
                reference: competition.payment_reference.clone(),
            },
            jury: competition.jury.as_ref().map(|jury| JuryConfig {
                min_age: jury.min_age,
                deployments_per_two_acts: jury.deployments_per_two_acts,
                missing_deployment_fee: jury.missing_deployment_fee,
                registration_end: jury.registration_end,
            }),
            features: competition.features.clone(),
            photos: competition.photos.clone(),
            // The logo isn't part of the competition's row (see
            // `crate::logo`).
            logo_svg: Branding::Keep,
            // A default color is left out, like a missing logo.
            primary_color: competition
                .primary_color
                .clone()
                .map_or(Branding::Keep, Branding::Set),
            secondary_color: competition
                .secondary_color
                .clone()
                .map_or(Branding::Keep, Branding::Set),
        }
    }
}

/// Inserts the competition from the competition file or updates it if it
/// already exists.
pub async fn upsert_competition(
    db: impl sqlx::SqliteExecutor<'_>,
    config: &CompetitionConfig,
) -> sqlx::Result<()> {
    let jury = config.jury.as_ref();
    let photos = config.photos.as_ref();
    sqlx::query!(
        r#"
        INSERT INTO competition (
            id, name, location, start_date, end_date, determining_day,
            registration_start, registration_end, music_upload_end,
            fee_per_start, max_fee_per_rider, venue, contact_email, announcement_url, photos_url,
            payment_deadline, payment_account_holder, payment_iban, payment_bic, payment_reference,
            jury_min_age, jury_deployments_per_two_acts, jury_missing_deployment_fee,
            jury_registration_end, feature_jury_registration, feature_moderation_input,
            photos_source, photos_user, photos_password
        ) VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
        )
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
            jury_registration_end = excluded.jury_registration_end,
            feature_jury_registration = excluded.feature_jury_registration,
            feature_moderation_input = excluded.feature_moderation_input,
            photos_source = excluded.photos_source,
            photos_user = excluded.photos_user,
            photos_password = excluded.photos_password
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
        config.features.jury_registration,
        config.features.moderation_input,
        photos.map(|photos| &photos.source),
        photos.and_then(|photos| photos.user.as_ref()),
        photos.and_then(|photos| photos.password.as_ref()),
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Whether `date` is one of the open birthday bounds (1900-01-01 and
/// 9999-12-31), which aren't tied to the determining day.
pub fn is_open_birthday_bound(date: Date) -> bool {
    date == crate::seed::OPEN_FROM_BIRTHDAY || date == crate::seed::OPEN_TO_BIRTHDAY
}

/// Where a category's birthday bound lands when the determining day moves
/// from `old` to `new`: the same year at the new day and month, if the bound
/// looks like it was computed from `old` (UTC midnight, same day and month).
/// `None` if it stays as it is. The open bounds never move.
pub fn move_birthday_bound(bound: OffsetDateTime, old: Date, new: Date) -> Option<OffsetDateTime> {
    let date = bound.date();
    let midnight = bound.offset().is_utc() && bound.time() == time::Time::MIDNIGHT;
    if is_open_birthday_bound(date)
        || !midnight
        || date.month() != old.month()
        || date.day() != old.day()
    {
        return None;
    }
    let moved = Date::from_calendar_date(date.year(), new.month(), new.day()).ok()?;
    Some(moved.midnight().assume_utc())
}

/// Parses a stored birthday bound: RFC 3339 as the competition file's
/// import writes it, or a bare date as the category form sends it.
pub fn parse_birthday_bound(text: &str) -> Option<OffsetDateTime> {
    use time::{format_description::well_known::Rfc3339, macros::format_description};
    OffsetDateTime::parse(text, &Rfc3339).ok().or_else(|| {
        Date::parse(text, format_description!("[year]-[month]-[day]"))
            .ok()
            .map(|date| date.midnight().assume_utc())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_features_follow_the_system_config_then_the_build_default() {
        let on = Features {
            jury_registration: Some(true),
            moderation_input: Some(true),
        };
        let off = Features {
            jury_registration: Some(false),
            moderation_input: Some(false),
        };
        let unset = Features::default();
        assert!(on.resolve(&off).jury_registration);
        assert!(!off.resolve(&on).jury_registration);
        assert!(unset.resolve(&on).jury_registration);
        assert!(!unset.resolve(&off).jury_registration);
        assert!(on.resolve(&off).moderation_input);
        assert!(!off.resolve(&on).moderation_input);
        assert!(unset.resolve(&on).moderation_input);
        assert!(!unset.resolve(&off).moderation_input);
        assert_eq!(
            unset.resolve(&unset).jury_registration,
            cfg!(debug_assertions)
        );
        assert!(unset.resolve(&unset).moderation_input);
        assert!(Features::of(None, &on).jury_registration);
        assert!(Features::of(None, &on).moderation_input);
    }
}
