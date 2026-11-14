//! The database contract: the [`Db`] trait, the shared ID types and errors.
//!
//! This crate has no database driver. Backends (`db-sqlite`, later
//! `db-postgres`) implement [`Db`]; the application talks to a [`DbHandle`].

use std::sync::Arc;

pub mod auth;
pub mod ids;
pub mod roles;
pub mod settings;
pub mod users;

pub use auth::AuthRepo;
pub use settings::SettingsRepo;
pub use users::UsersRepo;

/// What every database backend provides: the per-domain repository traits
/// (#238) as supertraits, plus the backend's own housekeeping.
///
/// The traits speak in domain types. They never expose a transaction, a pool
/// or a driver type: one method is one unit of work, and the implementation
/// decides how to make it atomic.
#[async_trait::async_trait]
pub trait Db: AuthRepo + UsersRepo + SettingsRepo + Send + Sync + 'static {
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
