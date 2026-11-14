//! The `competition` row: reading the current competition and writing its
//! settings. Backend internals; the application goes through
//! [`db_core::CompetitionRepo`] and [`db_core::SettingsRepo`].
//!
//! `current`, `upsert` and `write_colors` are `pub` only for the seed, demo
//! and export code that still holds a connection (#238, step D8b).

use db_core::{
    branding::Branding,
    colors::HexColor,
    competition::{Competition, Features, JuryRules, PhotosSettings},
};
use time::{Date, OffsetDateTime};

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

/// Inserts the competition or updates it if it already exists.
pub async fn upsert(db: impl sqlx::SqliteExecutor<'_>, config: &Competition) -> sqlx::Result<()> {
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
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29
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
        config.payment_deadline,
        config.payment_account_holder,
        config.payment_iban,
        config.payment_bic,
        config.payment_reference,
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

/// Writes the competition's colors: each is kept, reset to the default or
/// set (ADR 0009).
pub async fn write_colors(
    db: impl sqlx::SqliteExecutor<'_>,
    competition_id: &str,
    primary: &Branding<HexColor>,
    secondary: &Branding<HexColor>,
) -> sqlx::Result<()> {
    let value = |color: &Branding<HexColor>| color.value().map(HexColor::to_string);
    let (keep_primary, primary) = (primary.is_keep(), value(primary));
    let (keep_secondary, secondary) = (secondary.is_keep(), value(secondary));
    sqlx::query!(
        r#"
        UPDATE competition SET
            primary_color = IIF($1, primary_color, $2),
            secondary_color = IIF($3, secondary_color, $4)
        WHERE id = $5
        "#,
        keep_primary,
        primary,
        keep_secondary,
        secondary,
        competition_id,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// The id of the current competition; `None` without one.
pub(crate) async fn current_id(db: impl sqlx::SqliteExecutor<'_>) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar!(r#"SELECT id as "id!" FROM competition ORDER BY start_date DESC LIMIT 1"#)
        .fetch_optional(db)
        .await
}
