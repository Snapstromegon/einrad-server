//! The competition's settings, as the database stores them: the
//! [`Competition`] row, its feature switches, and the [`CompetitionRepo`]
//! trait to read and change them.

use time::{Date, OffsetDateTime};

use crate::{DbError, colors::HexColor};

time::serde::format_description!(pub iso_date, Date, "[year]-[month]-[day]");

/// Where the info board's photos come from, as the competition sets it
/// (`[competition.photos]`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PhotosSettings {
    /// A WebDAV URL, a Nextcloud share link or a local folder.
    pub source: String,
    /// Nextcloud user for a WebDAV folder.
    #[serde(default)]
    pub user: Option<String>,
    /// Nextcloud app password for a WebDAV folder, or a share link's
    /// password.
    #[serde(default)]
    pub password: Option<String>,
}

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

/// Features that are still being refined, as a competition sets them: on,
/// off, or unset (the build default).
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

    /// Fills in what's unset with each feature's build default: the jury
    /// registration is on, the moderation input is off.
    pub fn resolve(&self) -> ResolvedFeatures {
        ResolvedFeatures {
            jury_registration: self.jury_registration.unwrap_or(true),
            moderation_input: self.moderation_input.unwrap_or(false),
        }
    }

    /// The features of `competition`, or only the build defaults without one.
    pub fn of(competition: Option<&Competition>) -> ResolvedFeatures {
        competition
            .map(|competition| competition.features.clone())
            .unwrap_or_default()
            .resolve()
    }

    /// The features of the current competition, read through the database
    /// trait. Read per request, so a change applies without a restart.
    pub async fn from_repo(repo: &dyn crate::Db) -> Result<ResolvedFeatures, crate::DbError> {
        let flags = repo.feature_flags().await?;
        Ok(Features {
            jury_registration: flags.jury_registration,
            moderation_input: flags.moderation_input,
        }
        .resolve())
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
    /// End of the judge registration: the jury's own deadline, or the
    /// registration's end.
    pub fn judge_registration_end(&self) -> OffsetDateTime {
        self.jury
            .as_ref()
            .and_then(|jury| jury.registration_end)
            .unwrap_or(self.registration_end)
    }
}

/// Reading and editing the current competition.
#[async_trait::async_trait]
pub trait CompetitionRepo {
    /// The current competition: the one with the latest start date.
    async fn current_competition(&self) -> Result<Option<Competition>, DbError>;

    /// Writes the edited settings of the current competition and both
    /// colors (`None` is the default), in one transaction. `false` if there
    /// is no current competition, or its id isn't `edited.id`.
    async fn save_competition_settings(&self, edited: &Competition) -> Result<bool, DbError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_features_follow_the_build_default() {
        let on = Features {
            jury_registration: Some(true),
            moderation_input: Some(true),
        };
        let off = Features {
            jury_registration: Some(false),
            moderation_input: Some(false),
        };
        let unset = Features::default();
        assert!(on.resolve().jury_registration);
        assert!(on.resolve().moderation_input);
        assert!(!off.resolve().jury_registration);
        assert!(!off.resolve().moderation_input);
        assert!(unset.resolve().jury_registration);
        assert!(!unset.resolve().moderation_input);
        assert_eq!(Features::of(None), unset.resolve());
    }
}
