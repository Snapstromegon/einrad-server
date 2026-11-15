//! The jury registration (#225): clubs register their judges (JurorInnen)
//! for the heats' Vorlaufs and the finals (jury units), admins check each club's jury
//! obligation and plan the jury. Everything is behind the
//! `jury_registration` feature of the competition (see `Features`).
//!
//! Types and rules live in `db_core::jury`; the commands and queries in
//! `db_core::{commands, queries}::jury`, the SQL in the backend.

use axum::{extract::FromRequestParts, http::request::Parts};
use db_core::DbHandle;

pub use db_core::jury::*;

use crate::{competition::Features, http_server::HttpError};

/// Guard for every jury endpoint: they don't exist (404) while the feature
/// is off. Read per request, so turning it on or off applies immediately.
#[derive(Debug)]
pub struct JuryEnabled;

impl<S: Send + Sync> FromRequestParts<S> for JuryEnabled {
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Some(repo) = parts.extensions.get::<DbHandle>() else {
            return Err(HttpError::NotFound);
        };
        if Features::from_repo(repo.as_ref()).await?.jury_registration {
            Ok(JuryEnabled)
        } else {
            Err(HttpError::NotFound)
        }
    }
}
