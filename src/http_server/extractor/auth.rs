use std::{marker::PhantomData, ops::Deref, sync::Arc};

use axum::{
    extract::{FromRequestParts, OptionalFromRequestParts},
    http::{StatusCode, request::Parts},
    response::IntoResponse,
};
use axum_extra::extract::CookieJar;
use tracing::error;
use uuid::Uuid;

use crate::{
    http_server::HttpError,
    jwt::{JWTClaims, JWTConfig},
    reloadable_sqlite::ReloadableSqlite,
    roles::{self, Permission, Permit, Role},
};

#[derive(Debug)]
pub struct Auth {
    pub user_id: Uuid,
    pub email: String,
    pub name: String,
    /// Sorted.
    pub roles: Vec<Role>,
    pub club_id: Option<Uuid>,
    pub email_verified: bool,
}

impl Auth {
    pub fn can(&self, permission: Permission) -> bool {
        self.roles
            .iter()
            .any(|r| r.permissions().contains(&permission))
    }

    /// Sorted.
    pub fn permissions(&self) -> Vec<Permission> {
        roles::permissions_of(&self.roles)
    }
}

/// A logged-in user holding the permission `P`: without a login the request
/// is rejected with 401, without the permission with 403. As an `Option`,
/// it's `None` for both, for handlers whose answer depends on the viewer.
///
/// ```ignore
/// async fn set_payment(_: Granted<permit::ManagePayments>, …)
/// ```
pub struct Granted<P> {
    pub auth: Auth,
    permit: PhantomData<P>,
}

impl<P> std::fmt::Debug for Granted<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Granted")
            .field("permission", &std::any::type_name::<P>())
            .field("auth", &self.auth)
            .finish()
    }
}

impl<P> Deref for Granted<P> {
    type Target = Auth;

    fn deref(&self) -> &Auth {
        &self.auth
    }
}

impl<P: Permit> Granted<P> {
    fn check(auth: Auth) -> Result<Self, Error> {
        if auth.can(P::PERMISSION) {
            Ok(Self {
                auth,
                permit: PhantomData,
            })
        } else {
            Err(Error::PermissionMissing)
        }
    }
}

impl<S: Send + Sync, P: Permit> FromRequestParts<S> for Granted<P> {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Self::check(<Auth as FromRequestParts<S>>::from_request_parts(parts, state).await?)
    }
}

impl<S: Send + Sync, P: Permit> OptionalFromRequestParts<S> for Granted<P> {
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        let auth = <Auth as OptionalFromRequestParts<S>>::from_request_parts(parts, state).await?;
        Ok(auth.and_then(|auth| Self::check(auth).ok()))
    }
}

impl<S> FromRequestParts<S> for Auth
where
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        <Auth as OptionalFromRequestParts<S>>::from_request_parts(parts, state)
            .await?
            .ok_or(Error::JwtMissing)
    }
}

impl<S: Send + Sync> OptionalFromRequestParts<S> for Auth {
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        let cookies = CookieJar::from_request_parts(parts, state)
            .await
            .or(Err(Error::CookiesMissing))?;

        if let Some(jwt) = cookies.get("jwt") {
            let jwt_config = parts.extensions.get::<Arc<JWTConfig>>().unwrap();

            let token_data = jsonwebtoken::decode::<JWTClaims>(
                jwt.value_trimmed(),
                jwt_config.decode_key(),
                jwt_config.validation(),
            )
            .map_err(|_| Error::JwtInvalid)?;
            let user_id = token_data.claims.sub();
            let db = parts.extensions.get::<ReloadableSqlite>().unwrap();
            let user = sqlx::query!(
                r#"
                SELECT email, name, club_id as "club_id: Uuid", email_verified FROM users WHERE id = ?
                "#,
                user_id,
            )
            .fetch_optional(&db.get().await.clone())
            .await
            .map_err(|e| {
                error!("Couldn't load the user: {e}");
                Error::Database
            })?
            .ok_or(Error::UserNotFound)?;
            let roles = roles::roles_of(&db.get().await.clone(), user_id)
                .await
                .map_err(|e| {
                    error!("Couldn't load the user's roles: {e}");
                    Error::Database
                })?;
            Ok(Some(Auth {
                user_id,
                email: user.email,
                name: user.name,
                roles,
                club_id: user.club_id,
                email_verified: user.email_verified,
            }))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Logged in, but without the permission the request needs.
    #[error("Permission missing")]
    PermissionMissing,
    #[error("Cookies missing")]
    CookiesMissing,
    #[error("JWT missing")]
    JwtMissing,
    #[error("JWT invalid")]
    JwtInvalid,
    /// The token is valid, but its user was deleted.
    #[error("User not found")]
    UserNotFound,
    /// The user couldn't be loaded, e.g. because the database is busy.
    #[error("Database error")]
    Database,
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response<axum::body::Body> {
        let status = match self {
            // The same body as other 403s of the API.
            Error::PermissionMissing => {
                return HttpError::StatusCode(StatusCode::FORBIDDEN).into_response();
            }
            Error::CookiesMissing | Error::JwtInvalid | Error::JwtMissing | Error::UserNotFound => {
                StatusCode::UNAUTHORIZED
            }
            Error::Database => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, self.to_string()).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_errors_map_to_expected_status_codes() {
        let cases = [
            (Error::PermissionMissing, StatusCode::FORBIDDEN),
            (Error::CookiesMissing, StatusCode::UNAUTHORIZED),
            (Error::JwtMissing, StatusCode::UNAUTHORIZED),
            (Error::JwtInvalid, StatusCode::UNAUTHORIZED),
            (Error::UserNotFound, StatusCode::UNAUTHORIZED),
            (Error::Database, StatusCode::INTERNAL_SERVER_ERROR),
        ];
        for (error, expected) in cases {
            let message = error.to_string();
            assert_eq!(error.into_response().status(), expected, "{message}");
        }
    }
}
