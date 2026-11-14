//! Shared harness for integration tests.
//!
//! Every test gets a fresh, migrated SQLite database in memory and
//! drives the real API router in-process, without binding a socket. Tests that
//! send mails additionally get their own Mailpit container.
#![allow(dead_code)]

pub mod mailpit;

/// Every test gets its own database in memory.
#[allow(unused_imports)]
pub use einrad_server::reloadable_sqlite::{in_memory, in_memory_migrated};

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use db_sqlite::SqliteRepo;
use einrad_server::ids::{ActId, CategoryId, ClubId, FinalId, HeatId, RiderId, UserId};
use einrad_server::{
    config::CompetitionFile,
    http_server::{HttpServerOptions, routes::get_api_router},
    jwt::JWTConfig,
    mailer::Mailer,
    photos::{Photos, PhotosSettings},
    player::PlayerHub,
    reloadable_sqlite::ReloadableSqlite,
    seed,
    system_status::StatusOptions,
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Validation};
use password_auth::generate_hash;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;
use uuid::Uuid;

use mailpit::{FROM_ADDRESS, Mail, Mailpit};

pub const BASE_URL: &str = "http://cup.test";
pub const METRICS_TOKEN: &str = "test-metrics-token";
pub const RELOAD_DB_TOKEN: &str = "test-reload-token";
pub const PASSWORD: &str = "Sicheres-Passwort1";

pub struct TestApp {
    pub db: SqlitePool,
    pub repo: db_core::DbHandle,
    pub jwt: Arc<JWTConfig>,
    mailpit: Option<Mailpit>,
    data_dir: TempDir,
    router: Router,
    pub player_hub: Arc<PlayerHub>,
    pub photos: Arc<Photos>,
}

/// Directory removed again when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!("cup-test-data-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Value,
}

/// A response whose body stays bytes; see [`TestApp::get_raw`].
pub struct RawResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
}

impl TestResponse {
    /// Returns the `name=value` pair of the `jwt` cookie set by this response.
    pub fn jwt_cookie(&self) -> Option<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|v| v.starts_with("jwt="))
            .map(|v| v.split(';').next().unwrap().to_string())
    }
}

/// Registration window that is currently open.
pub fn open_registration() -> StatusOptions {
    let now = OffsetDateTime::now_utc();
    StatusOptions {
        start_register_date: now - Duration::days(1),
        end_register_date: now + Duration::days(1),
        end_music_upload_date: now + Duration::days(1),
        end_judge_register_date: None,
    }
}

/// Registration window that has already ended.
pub fn closed_registration() -> StatusOptions {
    let now = OffsetDateTime::now_utc();
    StatusOptions {
        start_register_date: now - Duration::days(10),
        end_register_date: now - Duration::days(1),
        end_music_upload_date: now - Duration::days(1),
        end_judge_register_date: None,
    }
}

/// Seeds the competition, categories, finals and timeplan from
/// `tests/fixtures/competition.toml` and sets the registration dates of the
/// competition to `status`.
pub async fn seed_db(db: &SqlitePool, status: &StatusOptions) {
    let file = CompetitionFile::parse(include_str!("../fixtures/competition.toml")).unwrap();
    let data = TempDir::new();
    seed::apply(db, Some(&file), false, &data.0).await.unwrap();
    set_registration_status(db, status).await;
    // Tests read the startlist unless they are about its publication.
    set_startlist_published(db, true).await;
}

/// Publishes the competition's startlist and timeplan, or takes them back.
pub async fn set_startlist_published(db: &SqlitePool, published: bool) {
    sqlx::query("UPDATE competition SET startlist_published_at = $1")
        .bind(published.then(OffsetDateTime::now_utc))
        .execute(db)
        .await
        .unwrap();
}

/// Sets the competition's photo settings.
pub async fn set_photos(db: &SqlitePool, photos: Option<&PhotosSettings>) {
    sqlx::query(
        "UPDATE competition SET photos_source = $1, photos_user = $2, photos_password = $3",
    )
    .bind(photos.map(|p| p.source.clone()))
    .bind(photos.and_then(|p| p.user.clone()))
    .bind(photos.and_then(|p| p.password.clone()))
    .execute(db)
    .await
    .unwrap();
}

/// Sets the competition's `jury_registration` feature: on, off or unset.
pub async fn set_jury_registration(db: &SqlitePool, on: Option<bool>) {
    sqlx::query("UPDATE competition SET feature_jury_registration = $1")
        .bind(on)
        .execute(db)
        .await
        .unwrap();
}

/// Sets the competition's `moderation_input` feature: on, off or unset.
pub async fn set_moderation_input(db: &SqlitePool, on: Option<bool>) {
    sqlx::query("UPDATE competition SET feature_moderation_input = $1")
        .bind(on)
        .execute(db)
        .await
        .unwrap();
}

/// Sets the registration dates of the competition to `status`. Capabilities
/// are read per request, so this takes effect immediately.
pub async fn set_registration_status(db: &SqlitePool, status: &StatusOptions) {
    sqlx::query(
        "UPDATE competition SET registration_start = $1, registration_end = $2, music_upload_end = $3",
    )
    .bind(status.start_register_date)
    .bind(status.end_register_date)
    .bind(status.end_music_upload_date)
    .execute(db)
    .await
    .unwrap();
}

impl TestApp {
    /// The id of the heat with the code `name`: the admin API addresses
    /// heats by it.
    pub async fn heat_id(&self, name: &str) -> HeatId {
        sqlx::query_scalar("SELECT id FROM heats WHERE code = $1")
            .bind(name)
            .fetch_one(&self.db)
            .await
            .unwrap_or_else(|e| panic!("no heat {name}: {e}"))
    }

    /// The id of the final with the code `name`: the admin API addresses
    /// finals by it.
    pub async fn final_id(&self, name: &str) -> FinalId {
        sqlx::query_scalar("SELECT id FROM finals WHERE code = $1")
            .bind(name)
            .fetch_one(&self.db)
            .await
            .unwrap_or_else(|e| panic!("no final {name}: {e}"))
    }

    /// App without a mail server; sending a mail makes the request fail.
    pub async fn new(db: SqlitePool) -> Self {
        Self::with_status(db, open_registration()).await
    }

    pub async fn with_status(db: SqlitePool, status: StatusOptions) -> Self {
        seed_db(&db, &status).await;
        // Nothing listens on port 1, so any attempt to send a mail fails.
        let mailer = Mailer::from_url("smtp://127.0.0.1:1", FROM_ADDRESS).unwrap();
        Self::build(db, mailer, None, None)
    }

    /// App without a competition: the server in setup mode.
    pub async fn without_competition(db: SqlitePool) -> Self {
        let mailer = Mailer::from_url("smtp://127.0.0.1:1", FROM_ADDRESS).unwrap();
        Self::build(db, mailer, None, None)
    }

    /// App whose competition turns the jury registration off, as release
    /// builds do for a competition that leaves it unset.
    pub async fn without_jury_registration(db: SqlitePool) -> Self {
        seed_db(&db, &open_registration()).await;
        set_jury_registration(&db, Some(false)).await;
        let mailer = Mailer::from_url("smtp://127.0.0.1:1", FROM_ADDRESS).unwrap();
        Self::build(db, mailer, None, None)
    }

    /// App showing the photos of `photos` on the info board: the
    /// competition's photo settings. [`TestApp::sync_photos`] lists them.
    pub async fn with_photos(db: SqlitePool, photos: PhotosSettings) -> Self {
        seed_db(&db, &open_registration()).await;
        set_photos(&db, Some(&photos)).await;
        let mailer = Mailer::from_url("smtp://127.0.0.1:1", FROM_ADDRESS).unwrap();
        Self::build(db, mailer, None, None)
    }

    /// App sending mails to a dedicated Mailpit container.
    pub async fn with_mailpit(db: SqlitePool) -> Self {
        seed_db(&db, &open_registration()).await;
        let mailpit = Mailpit::start().await;
        let mailer = mailpit.mailer();
        Self::build(db, mailer, Some(mailpit), None)
    }

    /// App started with a competition file whose id is `file_id`.
    pub async fn started_with_competition_file(db: SqlitePool, file_id: &str) -> Self {
        seed_db(&db, &open_registration()).await;
        let mailer = Mailer::from_url("smtp://127.0.0.1:1", FROM_ADDRESS).unwrap();
        Self::build(db, mailer, None, Some(file_id))
    }

    /// App without a `reload_db_token`: the token path of `reload_db` is off.
    pub async fn without_reload_token(db: SqlitePool) -> Self {
        seed_db(&db, &open_registration()).await;
        let mailer = Mailer::from_url("smtp://127.0.0.1:1", FROM_ADDRESS).unwrap();
        Self::build_with(db, mailer, None, None, None, "")
    }

    /// App whose database file (as far as the server knows) is `db.sqlite`
    /// in `dir`, so backups land there. The pool itself stays in memory.
    pub async fn with_db_dir(db: SqlitePool, dir: &Path) -> Self {
        seed_db(&db, &open_registration()).await;
        let mailer = Mailer::from_url("smtp://127.0.0.1:1", FROM_ADDRESS).unwrap();
        let url = format!("sqlite://{}", dir.join("db.sqlite").display());
        Self::build_with(db, mailer, None, None, Some(RELOAD_DB_TOKEN), &url)
    }

    fn build(
        db: SqlitePool,
        mailer: Mailer,
        mailpit: Option<Mailpit>,
        competition_file_id: Option<&str>,
    ) -> Self {
        Self::build_with(
            db,
            mailer,
            mailpit,
            competition_file_id,
            Some(RELOAD_DB_TOKEN),
            "",
        )
    }

    /// `competition_file_id` the id in the competition file the server
    /// started with, `reload_db_token` the system config's token.
    fn build_with(
        db: SqlitePool,
        mailer: Mailer,
        mailpit: Option<Mailpit>,
        competition_file_id: Option<&str>,
        reload_db_token: Option<&str>,
        db_url: &str,
    ) -> Self {
        let mut validation = Validation::new(Algorithm::HS512);
        validation.validate_aud = false;
        let jwt = Arc::new(JWTConfig::new(
            EncodingKey::from_secret(b"integration-test-secret"),
            DecodingKey::from_secret(b"integration-test-secret"),
            Algorithm::HS512,
            validation,
            true,
        ));
        let data_dir = TempDir::new();
        let options = HttpServerOptions {
            bind_address: "127.0.0.1:0".into(),
            base_url: BASE_URL.into(),
            data_path: data_dir.0.clone(),
            reload_db_token: reload_db_token.map(Into::into),
            metrics_token: Some(METRICS_TOKEN.into()),
            competition_file_id: competition_file_id.map(Into::into),
        };
        let reloadable = ReloadableSqlite::new(db.clone(), db_url.to_string());
        let repo = SqliteRepo::new(reloadable.clone()).into_handle();
        let player_hub = PlayerHub::new(repo.clone());
        let photos = Photos::new().unwrap();
        let router = get_api_router(
            Arc::new(options),
            reloadable,
            repo.clone(),
            Arc::new(mailer),
            jwt.clone(),
            player_hub.clone(),
            photos.clone(),
        );
        Self {
            db,
            repo,
            jwt,
            mailpit,
            data_dir,
            router,
            player_hub,
            photos,
        }
    }

    /// Takes the competition's photo settings and lists the source once,
    /// as the server does every few seconds.
    pub async fn sync_photos(&self) -> Result<(), String> {
        self.photos.sync(self.repo.as_ref()).await
    }

    /// Directory uploaded songs are stored in.
    pub fn data_path(&self) -> &Path {
        &self.data_dir.0
    }

    pub async fn request(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
        cookie: Option<&str>,
    ) -> TestResponse {
        match body {
            Some(body) => {
                self.send(
                    method,
                    uri,
                    Some("application/json"),
                    body.to_string().into_bytes(),
                    cookie,
                )
                .await
            }
            None => self.send(method, uri, None, Vec::new(), cookie).await,
        }
    }

    /// Uploads `contents` as a single multipart file field.
    pub async fn post_file(
        &self,
        uri: &str,
        file_name: &str,
        contents: &[u8],
        cookie: Option<&str>,
    ) -> TestResponse {
        self.post_file_with_fields(uri, &[], file_name, contents, cookie)
            .await
    }

    /// Uploads `contents` as a multipart file field after the text `fields`.
    pub async fn post_file_with_fields(
        &self,
        uri: &str,
        fields: &[(&str, &str)],
        file_name: &str,
        contents: &[u8],
        cookie: Option<&str>,
    ) -> TestResponse {
        let boundary = "cup-test-boundary";
        let mut body = String::new();
        for (name, value) in fields {
            body.push_str(&format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        let mut body = format!(
            "{body}--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(contents);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let content_type = format!("multipart/form-data; boundary={boundary}");
        self.send(Method::POST, uri, Some(&content_type), body, cookie)
            .await
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        content_type: Option<&str>,
        body: Vec<u8>,
        cookie: Option<&str>,
    ) -> TestResponse {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        if let Some(content_type) = content_type {
            builder = builder.header(header::CONTENT_TYPE, content_type);
        }
        let request = builder.body(Body::from(body)).unwrap();

        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        TestResponse {
            status,
            headers,
            body,
        }
    }

    pub async fn get(&self, uri: &str, cookie: Option<&str>) -> TestResponse {
        self.request(Method::GET, uri, None, cookie).await
    }

    /// A GET with extra request headers, e.g. `If-None-Match`, whose body
    /// stays bytes, e.g. an image.
    pub async fn get_raw(&self, uri: &str, headers: &[(header::HeaderName, &str)]) -> RawResponse {
        let mut builder = Request::builder().method(Method::GET).uri(uri);
        for (name, value) in headers {
            builder = builder.header(name, *value);
        }
        let request = builder.body(Body::empty()).unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        RawResponse {
            status,
            headers,
            bytes,
        }
    }

    /// Serves the app on a free local port, e.g. for websocket tests, with
    /// the songs at `/songs` like the real server. The server stops with the
    /// test.
    pub async fn serve(&self) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = self.router.clone().nest_service(
            "/songs",
            tower_http::services::ServeDir::new(self.data_path()),
        );
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        address
    }

    /// A GET with an `Authorization: Bearer` header, e.g. for `/metrics`.
    pub async fn get_with_bearer(&self, uri: &str, token: &str) -> TestResponse {
        let request = Request::builder()
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        TestResponse {
            status,
            headers,
            body: Value::String(String::from_utf8_lossy(&bytes).into_owned()),
        }
    }

    pub async fn post(&self, uri: &str, body: Value, cookie: Option<&str>) -> TestResponse {
        let body = self.categories_by_id(uri, body).await;
        self.request(Method::POST, uri, Some(body), cookie).await
    }

    /// The id of the category called `name`: the admin API addresses
    /// categories by it (#356).
    pub async fn category_id(&self, name: &str) -> CategoryId {
        sqlx::query_scalar("SELECT id FROM categories WHERE name = $1")
            .bind(name)
            .fetch_one(&self.db)
            .await
            .unwrap_or_else(|e| panic!("no category {name}: {e}"))
    }

    /// A shim that exists only to keep legacy tests readable (they name
    /// categories, the API takes IDs). The real request shape, with IDs, is
    /// covered per endpoint in `category_api.rs` and `category_ids.rs`. An
    /// unknown name panics, so a test can't silently test something else.
    ///
    /// Lets a test name categories where the admin API takes IDs: `post`
    /// swaps `category` (timeplan entries), `categories` (heats) and the
    /// `name` of `edit_category`, `delete_category` and the `move_category_*`
    /// bodies for their IDs, unless the body already carries them. Tests of
    /// the ID API itself (`category_ids.rs`) send IDs and skip this.
    async fn categories_by_id(&self, uri: &str, mut body: Value) -> Value {
        let endpoint = uri.rsplit('/').next().unwrap_or_default();
        let Some(object) = body.as_object_mut() else {
            return body;
        };
        match endpoint {
            "add_timeplan_entry" | "edit_timeplan_entry" => {
                if let Some(category) = object.remove("category")
                    && let Some(name) = category.as_str()
                {
                    let id = self.category_id(name).await;
                    object.insert("category_id".into(), json!(id));
                }
            }
            "add_heat" | "edit_heat" => {
                if let Some(Value::Array(names)) = object.remove("categories") {
                    let mut ids = vec![];
                    for name in names {
                        // Like the server did: names are trimmed, empty ones
                        // dropped.
                        let name = name.as_str().unwrap().trim();
                        if name.is_empty() {
                            continue;
                        }
                        ids.push(self.category_id(name).await);
                    }
                    object.insert("category_ids".into(), json!(ids));
                }
            }
            "edit_category" | "delete_category" | "move_category_up" | "move_category_down" => {
                if !object.contains_key("category_id")
                    && let Some(name) = object.remove("name")
                {
                    let id = self.category_id(name.as_str().unwrap()).await;
                    object.insert("category_id".into(), json!(id));
                }
            }
            _ => {}
        }
        body
    }

    /// Inserts a verified user directly into the database.
    pub async fn create_user(&self, name: &str, is_admin: bool) -> UserId {
        let roles: &[&str] = if is_admin { &["admin"] } else { &[] };
        self.create_user_with_roles(name, roles).await
    }

    /// Creates a user holding `roles`, e.g. `["treasurer"]`.
    pub async fn create_user_with_roles(&self, name: &str, roles: &[&str]) -> UserId {
        let id = UserId::new();
        sqlx::query(
            "INSERT INTO users (id, email, email_verified, name, password) VALUES ($1, $2, true, $3, $4)",
        )
        .bind(id)
        .bind(email_for(name))
        .bind(name)
        .bind(generate_hash(PASSWORD))
        .execute(&self.db)
        .await
        .unwrap();
        for role in roles {
            sqlx::query("INSERT INTO user_roles (user_id, role) VALUES ($1, $2)")
                .bind(id)
                .bind(role)
                .execute(&self.db)
                .await
                .unwrap();
        }
        id
    }

    /// Returns a `Cookie` header value authenticating as `user_id`.
    pub fn cookie_for(&self, user_id: UserId) -> String {
        format!("jwt={}", self.jwt.create_user_token(user_id).unwrap())
    }

    pub async fn create_admin(&self) -> String {
        let id = self.create_user("Admin", true).await;
        self.cookie_for(id)
    }

    /// Creates a user owning a freshly created club. Returns `(cookie, club_id)`.
    pub async fn create_club_owner(&self, name: &str, club_name: &str) -> (String, ClubId) {
        let cookie = self.cookie_for(self.create_user(name, false).await);
        let response = self
            .post(
                "/api/command/create_club",
                serde_json::json!({ "name": club_name }),
                Some(&cookie),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
        let club_id = response.body["club_id"].as_str().unwrap().parse().unwrap();
        (cookie, club_id)
    }

    /// Adds a rider from a [`rider_body`]: the rider's own fields with
    /// `add_rider`, then the starts (`single_male` / `single_female`, `pair`
    /// with `partner_id` or `partner_name`) with the start commands. A single
    /// gives the gender (Einzel männlich or weiblich).
    pub async fn add_rider(&self, cookie: &str, body: Value) -> RiderId {
        let flag = |key: &str| body[key].as_bool().unwrap_or(false);
        let gender = if flag("single_male") {
            json!("male")
        } else if flag("single_female") {
            json!("female")
        } else {
            body["gender"].clone()
        };
        let response = self
            .post(
                "/api/command/add_rider",
                json!({
                    "club_id": body["club_id"],
                    "firstname": body["firstname"],
                    "lastname": body["lastname"],
                    "birthdate": body["birthdate"],
                    "gender": gender,
                }),
                Some(cookie),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
        let rider_id: RiderId = response.body["rider_id"].as_str().unwrap().parse().unwrap();
        if flag("single_male") || flag("single_female") {
            self.start_command(
                cookie,
                "set_single",
                json!({ "rider_id": rider_id, "single": true }),
            )
            .await;
        }
        if flag("pair") {
            match &body["partner_id"] {
                Value::Null => {
                    self.start_command(
                        cookie,
                        "request_pair",
                        json!({ "rider_id": rider_id, "partner_name": body["partner_name"] }),
                    )
                    .await
                }
                partner_id => {
                    self.start_command(
                        cookie,
                        "link_pair",
                        json!({ "rider_id": rider_id, "partner_id": partner_id }),
                    )
                    .await
                }
            };
        }
        rider_id
    }

    /// Sends a start command (`set_single`, `request_pair`, `link_pair`,
    /// `leave_pair`) and asserts it succeeds.
    pub async fn start_command(&self, cookie: &str, command: &str, body: Value) -> Value {
        let response = self
            .post(&format!("/api/command/{command}"), body, Some(cookie))
            .await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{command}: {:?}",
            response.body
        );
        response.body
    }

    /// Adds a single rider and returns the id of their generated act.
    pub async fn add_single_act(&self, cookie: &str, club_id: ClubId, firstname: &str) -> ActId {
        let rider_id = self
            .add_rider(cookie, rider_body(club_id, firstname, true, false))
            .await;
        self.act_of(rider_id).await
    }

    /// Returns the id of the (single) act the rider participates in.
    pub async fn act_of(&self, rider_id: RiderId) -> ActId {
        sqlx::query_scalar("SELECT act_id FROM act_participants WHERE rider_id = $1")
            .bind(rider_id)
            .fetch_one(&self.db)
            .await
            .unwrap()
    }

    /// Returns the id of the act of `act_type` the rider takes part in.
    /// Unlike [`TestApp::act_of`] this stays unambiguous for riders in groups.
    pub async fn act_of_type(&self, rider_id: RiderId, act_type: &str) -> ActId {
        sqlx::query_scalar(
            "SELECT act_id FROM act_participants JOIN acts ON acts.id = act_id WHERE rider_id = $1 AND act_type = $2",
        )
        .bind(rider_id)
        .bind(act_type)
        .fetch_one(&self.db)
        .await
        .unwrap()
    }

    /// Adds `count` riders without single or pair starts, named
    /// `{prefix}1 Muster`, `{prefix}2 Muster`, …, and returns their ids.
    pub async fn add_riders(
        &self,
        cookie: &str,
        club_id: ClubId,
        prefix: &str,
        count: usize,
    ) -> Vec<RiderId> {
        let mut ids = Vec::with_capacity(count);
        for i in 1..=count {
            let body = rider_body(club_id, &format!("{prefix}{i}"), false, false);
            ids.push(self.add_rider(cookie, body).await);
        }
        ids
    }

    /// Creates a small group (Kleingruppe) owned by `club_id` and returns its
    /// act id.
    pub async fn create_group(&self, cookie: &str, club_id: ClubId, group_name: &str) -> ActId {
        self.create_group_of_type(cookie, club_id, group_name, "group_small")
            .await
    }

    /// Creates a large group (Großgruppe) owned by `club_id` and returns its
    /// act id.
    pub async fn create_large_group(
        &self,
        cookie: &str,
        club_id: ClubId,
        group_name: &str,
    ) -> ActId {
        self.create_group_of_type(cookie, club_id, group_name, "group_large")
            .await
    }

    /// Creates a group of `act_type` owned by `club_id` and returns its act id.
    pub async fn create_group_of_type(
        &self,
        cookie: &str,
        club_id: ClubId,
        group_name: &str,
        act_type: &str,
    ) -> ActId {
        let response = self
            .post(
                "/api/command/create_club_group",
                json!({ "club_id": club_id, "group_name": group_name, "act_type": act_type }),
                Some(cookie),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
        response.body["act_id"].as_str().unwrap().parse().unwrap()
    }

    /// Sets the riders of `club_id` in a group: `regular` riders and
    /// `replacements`. Returns the response, so failures can be asserted.
    pub async fn set_group_members(
        &self,
        cookie: &str,
        club_id: ClubId,
        act_id: ActId,
        regular: &[RiderId],
        replacements: &[RiderId],
    ) -> TestResponse {
        let members: Vec<Value> = regular
            .iter()
            .map(|id| json!({ "rider_id": id, "is_replacement": false }))
            .chain(
                replacements
                    .iter()
                    .map(|id| json!({ "rider_id": id, "is_replacement": true })),
            )
            .collect();
        self.post(
            "/api/command/set_club_group_members",
            json!({ "club_id": club_id, "act_id": act_id, "members": members }),
            Some(cookie),
        )
        .await
    }

    /// Ends the registration and music upload window, so data created while
    /// it was open can be tested with closed registration.
    pub async fn close_registration(&self) {
        set_registration_status(&self.db, &closed_registration()).await;
    }

    /// Returns all mails received by Mailpit, oldest first.
    pub async fn mails(&self) -> Vec<Mail> {
        self.mailpit
            .as_ref()
            .expect("create the app with TestApp::with_mailpit to inspect mails")
            .mails()
            .await
    }

    /// Returns the single mail received so far.
    pub async fn single_mail(&self) -> Mail {
        let mut mails = self.mails().await;
        assert_eq!(mails.len(), 1, "{mails:#?}");
        mails.pop().unwrap()
    }
}

pub fn email_for(name: &str) -> String {
    format!("{}@cup.test", name.to_lowercase().replace(' ', "."))
}

/// Body for [`TestApp::add_rider`]: the rider and their starts. The
/// birthdate places single female riders in the seeded `EWU13` category (12
/// years old on the determining day 2027-06-01).
pub fn rider_body(club_id: ClubId, firstname: &str, single: bool, pair: bool) -> Value {
    json!({
        "gender": "female",
        "club_id": club_id,
        "firstname": firstname,
        "lastname": "Muster",
        "birthdate": "2015-05-01T00:00:00.000Z",
        "single_male": false,
        "single_female": single,
        "pair": pair,
        "partner_id": null,
        "partner_name": null,
    })
}
