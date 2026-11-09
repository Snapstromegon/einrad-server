use axum::{
    extract::{FromRequestParts, OptionalFromRequestParts},
    http::{StatusCode, request::Parts},
};

use crate::{
    competition::Competition,
    http_server::{
        HttpError,
        extractor::auth::{Error, Granted},
    },
    reloadable_sqlite::ReloadableSqlite,
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
    let Some(db) = parts.extensions.get::<ReloadableSqlite>() else {
        return Err(HttpError::InternalServerError);
    };
    let db = db.get().await.clone();
    Ok(Competition::startlist_published(&db).await?)
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
