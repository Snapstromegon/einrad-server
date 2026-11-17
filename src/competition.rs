//! The competition's settings. The row and its rules live in `db-core`
//! (`CompetitionRepo` reads and edits them); this module adds the
//! conversions from and to the competition file.

pub use db_core::competition::{
    Competition, Features, JuryRules, PhotosSettings, ResolvedFeatures, iso_date,
};

use crate::{
    config::{Branding, CompetitionConfig, JuryConfig, PaymentConfig},
    system_status::StatusOptions,
};

impl From<&Competition> for StatusOptions {
    fn from(competition: &Competition) -> Self {
        StatusOptions {
            start_register_date: competition.registration_start,
            end_register_date: competition.registration_end,
            end_music_upload_date: competition.music_upload_end,
            end_judge_register_date: competition
                .jury
                .as_ref()
                .and_then(|jury| jury.registration_end),
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
            logo_url: Branding::Keep,
            break_image_svg: Branding::Keep,
            break_image_url: Branding::Keep,
            sponsors: None,
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
