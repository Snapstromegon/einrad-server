//! The database contract: the [`Db`] trait, the shared ID types and errors.
//!
//! This crate has no database driver. Backends (`db-sqlite`, later
//! `db-postgres`) implement [`Db`]; the application talks to a [`DbHandle`].

use std::sync::Arc;

pub mod act_type;
pub mod auth;
pub mod bookings;
pub mod check_in;
pub mod club_mails;
pub mod clubs;
pub mod fee_waivers;
pub mod fees;
pub mod gender;
pub mod groups;
pub mod ids;
pub mod jury;
pub mod people;
pub mod riders;
pub mod roles;
pub mod settings;
pub mod users;

pub use auth::AuthRepo;
pub use bookings::BookingsRepo;
pub use check_in::CheckInRepo;
pub use club_mails::ClubMailsRepo;
pub use clubs::ClubsRepo;
pub use fees::FeesRepo;
pub use groups::GroupsRepo;
pub use riders::RidersRepo;
pub use settings::SettingsRepo;
pub use users::UsersRepo;

/// What every database backend provides: the per-domain repository traits
/// (#238) as supertraits, plus the backend's own housekeeping.
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
    + CheckInRepo
    + ClubMailsRepo
    + Send
    + Sync
    + 'static
{
    /// Reopens the database after its file was replaced from the outside. A
    /// no-op for backends that have no file.
    async fn reload(&self) -> Result<(), DbError> {
        Ok(())
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
