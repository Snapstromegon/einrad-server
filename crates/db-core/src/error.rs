//! Why a command or query failed.

use crate::DbError;

/// Why a command or query failed. The messages of `Rejected`,
/// `NotFoundMessage` and `Conflict` are German and meant for the user. `Db`
/// covers infrastructure only.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("Not found")]
    NotFound,
    /// An entity the request names doesn't exist; the message is German.
    #[error("{0}")]
    NotFoundMessage(String),
    #[error("Forbidden")]
    Forbidden,
    /// The request is refused (today answered with 500, see the server's
    /// `From<CommandError> for HttpError`).
    #[error("{0}")]
    Rejected(String),
    /// The request clashes with the current state; the message is German.
    #[error("{0}")]
    Conflict(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

impl From<sqlx::Error> for CommandError {
    fn from(error: sqlx::Error) -> Self {
        Self::Db(error.into())
    }
}

/// Queries fail the same ways commands do.
pub type QueryError = CommandError;
