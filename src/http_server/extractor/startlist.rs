use axum::{
    extract::{FromRequestParts, OptionalFromRequestParts},
    http::{StatusCode, request::Parts},
};
use db_core::DbHandle;

use crate::{
    http_server::{
        HttpError,
        extractor::auth::{Auth, Error, Granted},
    },
    roles::permit,
};

/// Guard for everything that shows the start list or the timeplan: until an
/// admin published them, only the organizer team (permission
/// `ViewUnpublishedStartlist`) passes; everyone else gets a 403. Handlers
/// that can answer less instead of nothing take `Option<StartlistAccess>`.
/// Read per request, so publishing and revoking apply immediately.
#[derive(Debug)]
pub struct StartlistAccess;

async fn visible<S: Send + Sync>(parts: &mut Parts, state: &S) -> Result<bool, HttpError> {
    let staff = <Granted<permit::ViewUnpublishedStartlist> as OptionalFromRequestParts<S>>::from_request_parts(
        parts, state,
    )
    .await;
    match staff {
        Ok(Some(_)) => return Ok(true),
        Err(Error::Database) => return Err(HttpError::InternalServerError),
        // An expired or invalid login is no reason to turn a visitor away
        // from a published start list: they are just not staff.
        Ok(None) | Err(_) => {}
    }
    let Some(repo) = parts.extensions.get::<DbHandle>() else {
        return Err(HttpError::InternalServerError);
    };
    Ok(repo.startlist_published().await?)
}

impl<S: Send + Sync> FromRequestParts<S> for StartlistAccess {
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if visible(parts, state).await? {
            Ok(StartlistAccess)
        } else {
            // The same body as other 403s of the API.
            Err(HttpError::StatusCode(StatusCode::FORBIDDEN))
        }
    }
}

impl<S: Send + Sync> OptionalFromRequestParts<S> for StartlistAccess {
    type Rejection = HttpError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        Ok(visible(parts, state).await?.then_some(StartlistAccess))
    }
}

/// Guard for the list of all acts: until an admin published the start list
/// it is for logged-in users only (clubs register and manage their acts),
/// so visitors can't read who starts. Staff and a published list pass as in
/// [`StartlistAccess`].
#[derive(Debug)]
pub struct ActListAccess;

impl<S: Send + Sync> FromRequestParts<S> for ActListAccess {
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if visible(parts, state).await? {
            return Ok(ActListAccess);
        }
        match <Auth as OptionalFromRequestParts<S>>::from_request_parts(parts, state).await {
            Ok(Some(_)) => Ok(ActListAccess),
            Err(Error::Database) => Err(HttpError::InternalServerError),
            // No or an invalid login: a visitor.
            Ok(None) | Err(_) => Err(HttpError::StatusCode(StatusCode::FORBIDDEN)),
        }
    }
}
