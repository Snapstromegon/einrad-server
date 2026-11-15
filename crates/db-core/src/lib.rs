//! The database contract: the [`Db`] trait, the shared ID types and errors.
//!
//! This crate has no database driver. Backends (`db-sqlite`, later
//! `db-postgres`) implement [`Db`]; the application talks to a [`DbHandle`].

use std::sync::Arc;

pub mod act_type;
pub mod actor;
pub mod acts;
pub mod auth;
pub mod backups;
pub mod bookings;
pub mod branding;
pub mod category_ages;
pub mod club_mails;
pub mod clubs;
pub mod colors;
pub mod commands;
pub mod competition;
#[cfg(feature = "conformance")]
pub mod conformance;
pub mod error;
pub mod fee_waivers;
pub mod fees;
pub mod finals;
pub mod gender;
pub mod groups;
pub mod heats;
pub mod ids;
pub mod jury;
pub mod media;
pub mod metrics;
pub mod people;
pub mod prelude;
pub mod queries;
pub mod registration_rules;
pub mod riders;
pub mod roles;
pub mod seed;
pub mod settings;
pub mod structure_rules;
pub mod support;
pub mod timeplan;
pub mod users;

pub use acts::ActsRepo;
pub use auth::AuthRepo;
pub use bookings::BookingsRepo;
pub use club_mails::ClubMailsRepo;
pub use clubs::ClubsRepo;
pub use competition::CompetitionRepo;
pub use fees::FeesRepo;
pub use groups::GroupsRepo;
pub use jury::JuryRepo;
pub use media::MediaRepo;
pub use metrics::MetricsRepo;
pub use riders::RidersRepo;
pub use seed::{ExportRepo, SeedRepo};
pub use settings::SettingsRepo;
pub use timeplan::TimeplanRepo;
pub use users::UsersRepo;

/// What every database backend provides: the per-domain repository traits
/// (#238) as supertraits, plus the backend's own housekeeping.
///
/// The contract is moving to CQRS: [`commands::Commands`] and
/// [`queries::Queries`] (one method per API endpoint, grouped in small area
/// traits) and [`support::Support`] (non-endpoint callers) replace the `*Repo`
/// traits area by area; callers import [`prelude`]. The `*Repo` traits go away
/// at the end of that migration.
///
/// The traits speak in domain types. They never expose a transaction, a pool
/// or a driver type: one method is one unit of work, and the implementation
/// decides how to make it atomic.
#[async_trait::async_trait]
pub trait Db:
    AuthRepo
    + UsersRepo
    + SettingsRepo
    + ClubsRepo
    + RidersRepo
    + GroupsRepo
    + BookingsRepo
    + FeesRepo
    + ClubMailsRepo
    + ActsRepo
    + TimeplanRepo
    + JuryRepo
    + CompetitionRepo
    + MediaRepo
    + MetricsRepo
    + SeedRepo
    + ExportRepo
    + commands::Commands
    + queries::Queries
    + support::Support
    + Send
    + Sync
    + 'static
{
    /// Reopens the database after its file was replaced from the outside. A
    /// no-op for backends that have no file.
    async fn reload(&self) -> Result<(), DbError> {
        Ok(())
    }

    /// File backups and restores, for backends that keep their data in a
    /// file; `None` where there is nothing to back up that way.
    fn backups(&self) -> Option<&dyn backups::Backups> {
        None
    }
}

/// The shared handle the application passes around.
pub type DbHandle = Arc<dyn Db>;

/// Errors of the database layer.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
}
