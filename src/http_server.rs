use std::{path::PathBuf, sync::Arc};

use axum::{Json, response::IntoResponse};
use serde::Serialize;
use tracing::info;

use crate::{
    jwt::JWTConfig, mailer::Mailer, photos::Photos, player::PlayerHub,
    reloadable_sqlite::ReloadableSqlite,
};

pub mod extractor;
pub mod routes;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("Template error: {0}")]
    TemplateError(#[from] askama::Error),
    #[error("Not found")]
    NotFound,
    #[error("Internal server error")]
    InternalServerError,
    #[error("Database error: {0}")]
    DBError(#[from] sqlx::Error),
    #[error("Mail error: {0}")]
    MailError(#[from] crate::mailer::MailError),
    #[error("{0}")]
    ErrorMessages(String),
    /// The request clashes with the current state (409), with a German
    /// message for the user.
    #[error("{0}")]
    Conflict(String),
    #[error("Invalid credentials")]
    InvalidCredentials,
    #[error("Status code: {0}")]
    StatusCode(axum::http::StatusCode),
}

impl IntoResponse for HttpError {
    fn into_response(self) -> axum::response::Response {
        ClientError::from(self).into_response()
    }
}

#[derive(Debug, thiserror::Error, Serialize, utoipa::ToSchema)]
#[serde(tag = "type", content = "message")]
pub enum ClientError {
    #[error("Not found")]
    NotFound,
    #[error("Internal server error")]
    InternalServerError,
    #[error("Invalid credentials")]
    InvalidCredentials,
    #[error("{0}")]
    Generic(String),
    /// The request clashes with the current state; the message is German.
    #[error("{0}")]
    Conflict(String),
    #[error("Status code: {0}")]
    StatusCode(u16),
}

impl From<HttpError> for ClientError {
    fn from(e: HttpError) -> Self {
        match e {
            HttpError::TemplateError(e) => ClientError::Generic(format!("Template error: {:?}", e)),
            HttpError::NotFound => ClientError::NotFound,
            HttpError::InternalServerError => ClientError::InternalServerError,
            HttpError::DBError(e) => ClientError::Generic(format!("Database error: {:?}", e)),
            HttpError::MailError(e) => ClientError::Generic(format!("Mail error: {:?}", e)),
            HttpError::ErrorMessages(e) => ClientError::Generic(format!("{e:?}")),
            HttpError::Conflict(e) => ClientError::Conflict(e),
            HttpError::InvalidCredentials => ClientError::InvalidCredentials,
            HttpError::StatusCode(code) => ClientError::StatusCode(code.as_u16()),
        }
    }
}

impl IntoResponse for ClientError {
    fn into_response(self) -> axum::http::Response<axum::body::Body> {
        (
            match &self {
                Self::NotFound => axum::http::StatusCode::NOT_FOUND,
                Self::InternalServerError => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Self::Generic(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Self::Conflict(_) => axum::http::StatusCode::CONFLICT,
                Self::InvalidCredentials => axum::http::StatusCode::UNAUTHORIZED,
                Self::StatusCode(code) => axum::http::StatusCode::from_u16(*code).unwrap(),
            },
            Json(self),
        )
            .into_response()
    }
}

#[derive(Debug, Clone)]
pub struct HttpServerOptions {
    pub bind_address: String,
    pub base_url: String,
    pub data_path: PathBuf,
    /// Lets a logged-in non-admin reload the database; without one only
    /// admins can.
    pub reload_db_token: Option<String>,
    /// Bearer token for `GET /metrics`; without one the endpoint is off.
    pub metrics_token: Option<String>,
    /// The competition id in the competition file the server started with.
    pub competition_file_id: Option<String>,
}

pub struct HttpServer {
    db: ReloadableSqlite,
    mailer: Arc<Mailer>,
    jwt: Arc<JWTConfig>,
    options: Arc<HttpServerOptions>,
    player_hub: Arc<PlayerHub>,
    photos: Arc<Photos>,
}

impl HttpServer {
    pub fn new(
        options: HttpServerOptions,
        db: ReloadableSqlite,
        jwt: Arc<JWTConfig>,
        mailer: Arc<Mailer>,
        player_hub: Arc<PlayerHub>,
        photos: Arc<Photos>,
    ) -> Self {
        Self {
            db,
            mailer,
            options: Arc::new(options),
            jwt,
            player_hub,
            photos,
        }
    }

    /// The routes of the normal server, without a listener.
    pub fn router(&self) -> axum::Router {
        routes::get_router(
            self.options.clone(),
            self.db.clone(),
            self.mailer.clone(),
            self.jwt.clone(),
            self.player_hub.clone(),
            self.photos.clone(),
        )
    }

    pub fn start<F>(&mut self, shutdown_signal: F) -> tokio::task::JoinHandle<()>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let app = self.router();
        let address = self.options.bind_address.clone();

        tokio::spawn(async move {
            info!("Starting server on {}", address);
            let listener = tokio::net::TcpListener::bind(address).await.unwrap();
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal)
                .await
                .unwrap();
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    async fn response_parts(error: HttpError) -> (StatusCode, serde_json::Value) {
        let response = error.into_response();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn not_found_maps_to_404() {
        let (status, body) = response_parts(HttpError::NotFound).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, serde_json::json!({ "type": "NotFound" }));
    }

    #[tokio::test]
    async fn invalid_credentials_maps_to_401() {
        let (status, body) = response_parts(HttpError::InvalidCredentials).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, serde_json::json!({ "type": "InvalidCredentials" }));
    }

    #[tokio::test]
    async fn internal_server_error_maps_to_500() {
        let (status, body) = response_parts(HttpError::InternalServerError).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body, serde_json::json!({ "type": "InternalServerError" }));
    }

    #[tokio::test]
    async fn status_code_is_passed_through() {
        let (status, body) = response_parts(HttpError::StatusCode(StatusCode::FORBIDDEN)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            serde_json::json!({ "type": "StatusCode", "message": 403 })
        );
    }

    #[tokio::test]
    async fn error_messages_become_generic_500_with_message() {
        let (status, body) =
            response_parts(HttpError::ErrorMessages("Etwas ging schief".into())).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["type"], "Generic");
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("Etwas ging schief")
        );
    }

    #[tokio::test]
    async fn database_errors_become_generic_500() {
        let (status, body) = response_parts(HttpError::DBError(sqlx::Error::RowNotFound)).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["type"], "Generic");
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .starts_with("Database error:")
        );
    }
}
