use axum::{
    extract::{FromRequestParts, OptionalFromRequestParts},
    http::{StatusCode, request::Parts},
};

use crate::{
    http_server::{
        HttpError,
        extractor::auth::{Error, Granted},
    },
    jury,
    reloadable_sqlite::ReloadableSqlite,
    roles::permit,
};

/// Guard for the public Juryplan: until the Juryplan is published, only the
/// jury lead or organizer team passes.
#[derive(Debug)]
pub struct JuryPlanAccess;

async fn visible<S: Send + Sync>(parts: &mut Parts, state: &S) -> Result<bool, HttpError> {
    let staff = <Granted<permit::ManageJury> as OptionalFromRequestParts<S>>::from_request_parts(
        parts, state,
    )
    .await;
    match staff {
        Ok(Some(_)) => return Ok(true),
        Err(Error::Database) => return Err(HttpError::InternalServerError),
        Ok(None) | Err(_) => {}
    }
    let Some(db) = parts.extensions.get::<ReloadableSqlite>() else {
        return Err(HttpError::InternalServerError);
    };
    let db = db.get().await.clone();
    let mut conn = db.acquire().await?;
    Ok(jury::plan_published_at(&mut conn).await?.is_some())
}

impl<S: Send + Sync> FromRequestParts<S> for JuryPlanAccess {
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if visible(parts, state).await? {
            Ok(JuryPlanAccess)
        } else {
            Err(HttpError::StatusCode(StatusCode::FORBIDDEN))
        }
    }
}
