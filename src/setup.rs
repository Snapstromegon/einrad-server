//! First-run setup (ADR 0011): the server starts in setup instead of
//! refusing to start when its system config is incomplete or there is no
//! admin. This module decides that, makes the **setup token** and serves the
//! gate: the setup shell for every page, 503 for every other API.
//!
//! The gate is a router of its own with no database, mailer or JWT keys, so
//! nothing of the incomplete config is reachable by construction. The
//! completion and the handover to the normal server build on [`SetupState`].

use std::{
    collections::HashMap,
    fmt,
    future::Future,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{FromRequestParts, State},
    http::{HeaderValue, Method, StatusCode, Uri, header, request::Parts},
    response::{IntoResponse, Response},
    routing::get,
};
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use serde::Deserialize;
use sqlx::SqlitePool;
use subtle::ConstantTimeEq;
use tracing::{info, warn};
use url::Url;

use crate::{config::Settings, http_server::routes::StaticAssets, roles::Role};

/// The file the setup token is written to, next to the config path.
pub const TOKEN_FILE_NAME: &str = "einrad-server-setup-token.txt";
/// The setup page, which every page path serves while the setup is pending.
const SHELL: &str = "setup.html";
const SESSION_COOKIE: &str = "setup_session";
/// How long a setup cookie unlocks the setup API.
const SESSION_LIFETIME: Duration = Duration::from_secs(30 * 60);
const TOKEN_HEADER: &str = "x-setup-token";
const PENDING_MESSAGE: &str = "first-run setup pending";

/// Why the server is in first-run setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingReason {
    /// Required system settings are missing; the text lists them.
    IncompleteConfig(String),
    /// The database has no admin.
    NoAdmin,
}

impl fmt::Display for PendingReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IncompleteConfig(missing) => {
                write!(f, "the system config is incomplete. {missing}")
            }
            Self::NoAdmin => write!(f, "the database has no admin"),
        }
    }
}

/// Whether the server has to start in first-run setup: exactly when it
/// would otherwise refuse to start. `settings` is the result of
/// [`Settings::resolve`].
///
/// # Errors
/// If the database can't be read.
pub async fn pending_reason(
    settings: &Result<Settings, String>,
    db: &SqlitePool,
) -> sqlx::Result<Option<PendingReason>> {
    if let Err(missing) = settings {
        return Ok(Some(PendingReason::IncompleteConfig(missing.clone())));
    }
    let admins = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM user_roles WHERE role = ?"#,
        Role::Admin
    )
    .fetch_one(db)
    .await?;
    Ok((admins == 0).then_some(PendingReason::NoAdmin))
}

/// The directory of the config path (`--config`/`CONFIG`, default `./`),
/// where the token file goes.
#[must_use]
pub fn config_dir(config: Option<&Path>) -> PathBuf {
    match config.and_then(Path::parent) {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// 32 random bytes as hex.
fn random_secret() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the system has no source of randomness");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The token in `input`: the bare token, or the value of `token=` in a pasted
/// URL (or its query).
fn extract_token(input: &str) -> &str {
    let input = input.trim();
    match input.split_once("token=") {
        Some((_, rest)) => rest
            .split(|c: char| c == '&' || c == '#' || c.is_whitespace())
            .next()
            .unwrap_or(""),
        None => input,
    }
}

/// The state of a pending first-run setup, shared by the setup API.
#[derive(Debug)]
pub struct SetupState {
    token: String,
    base_url: Option<Url>,
    token_file: PathBuf,
    sessions: Mutex<HashMap<String, Instant>>,
}

impl SetupState {
    /// A new setup with a new token. `base_url` is the configured one, if
    /// any; the token file goes into `config_dir`.
    #[must_use]
    pub fn new(base_url: Option<Url>, config_dir: &Path) -> Arc<Self> {
        Arc::new(Self {
            token: random_secret(),
            base_url,
            token_file: config_dir.join(TOKEN_FILE_NAME),
            sessions: Mutex::new(HashMap::new()),
        })
    }

    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    #[must_use]
    pub fn token_file(&self) -> &Path {
        &self.token_file
    }

    /// What the file and the log show: the clickable setup URL if a base_url
    /// is configured, otherwise only the bare token.
    #[must_use]
    pub fn token_line(&self) -> String {
        match &self.base_url {
            Some(base) => format!(
                "{}/setup?token={}",
                base.as_str().trim_end_matches('/'),
                self.token
            ),
            None => self.token.clone(),
        }
    }

    /// Writes the token file (mode 0600), replacing an older one.
    ///
    /// # Errors
    /// If the file can't be written, e.g. the directory doesn't exist or isn't writable.
    pub fn write_token_file(&self) -> io::Result<()> {
        use std::io::Write;
        match std::fs::remove_file(&self.token_file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&self.token_file)?;
        writeln!(file, "{}", self.token_line())
    }

    /// Logs the token and writes the token file; if that fails, the token is
    /// only in the log.
    pub fn announce(&self, reason: &PendingReason) {
        warn!("Starting in first-run setup: {reason}.");
        match self.write_token_file() {
            Ok(()) => info!(
                "Setup token: {} (also in {})",
                self.token_line(),
                self.token_file.display()
            ),
            Err(e) => {
                warn!(
                    "Couldn't write the setup token file {}: {e}",
                    self.token_file.display()
                );
                info!("Setup token: {}", self.token_line());
            }
        }
    }

    /// Whether `input` (the bare token or a URL with `token=`) is the token.
    #[must_use]
    pub fn token_matches(&self, input: &str) -> bool {
        extract_token(input)
            .as_bytes()
            .ct_eq(self.token.as_bytes())
            .into()
    }

    /// Starts a session for a valid token; its id is what the cookie holds.
    fn open_session(&self, input: &str) -> Option<String> {
        if !self.token_matches(input) {
            return None;
        }
        let id = random_secret();
        let now = Instant::now();
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, expires| *expires > now);
        sessions.insert(id.clone(), now + SESSION_LIFETIME);
        Some(id)
    }

    fn session_valid(&self, id: &str) -> bool {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .is_some_and(|expires| *expires > Instant::now())
    }

    fn secure_cookie(&self) -> bool {
        self.base_url
            .as_ref()
            .is_some_and(|u| u.scheme() == "https")
    }
}

/// Proof that a request is unlocked by the setup token: the setup cookie, or
/// the token itself in the `x-setup-token` header. Every setup API call
/// beyond the token exchange takes it.
#[derive(Debug)]
pub struct SetupAuth;

impl FromRequestParts<Arc<SetupState>> for SetupAuth {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<SetupState>,
    ) -> Result<Self, Self::Rejection> {
        let by_header = parts
            .headers
            .get(TOKEN_HEADER)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| state.token_matches(v));
        let jar = CookieJar::from_headers(&parts.headers);
        let by_cookie = jar
            .get(SESSION_COOKIE)
            .is_some_and(|c| state.session_valid(c.value()));
        if by_header || by_cookie {
            Ok(Self)
        } else {
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

#[derive(Deserialize)]
struct TokenBody {
    token: String,
}

/// Exchanges the token for the setup cookie. The token stays valid, so a
/// reload or another tab can exchange it again.
async fn exchange_token(
    State(state): State<Arc<SetupState>>,
    jar: CookieJar,
    Json(body): Json<TokenBody>,
) -> Result<(CookieJar, StatusCode), StatusCode> {
    let id = state
        .open_session(&body.token)
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let cookie = Cookie::build((SESSION_COOKIE, id))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(state.secure_cookie())
        .path("/api/setup")
        .max_age(time::Duration::seconds(SESSION_LIFETIME.as_secs() as i64))
        .build();
    Ok((jar.add(cookie), StatusCode::NO_CONTENT))
}

/// 204 if the request is unlocked, otherwise 401.
async fn session(_auth: SetupAuth) -> StatusCode {
    StatusCode::NO_CONTENT
}

fn pending() -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, PENDING_MESSAGE).into_response()
}

/// Everything outside the setup API: the setup shell for pages, its assets,
/// 503 for every other API route.
async fn gate(method: Method, uri: Uri) -> Response {
    let path = uri.path();
    let is_api = ["/api", "/metrics", "/songs", "/swagger", "/openapi.json"]
        .iter()
        .any(|prefix| {
            path.strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        });
    if is_api || !matches!(method, Method::GET | Method::HEAD) {
        return pending();
    }
    let name = path.trim_start_matches('/');
    let asset = (name != "index.html" && name != SHELL)
        .then(|| StaticAssets::get(name))
        .flatten();
    let (file, mime) = match asset {
        Some(file) => {
            let mime = mime_guess::from_path(name)
                .first_or_octet_stream()
                .to_string();
            (file, mime)
        }
        None => match StaticAssets::get(SHELL) {
            Some(file) => (file, "text/html; charset=utf-8".to_string()),
            None => {
                warn!("static/{SHELL} is missing; build the frontend");
                return (StatusCode::INTERNAL_SERVER_ERROR, "setup page missing").into_response();
            }
        },
    };
    let mut response = (StatusCode::OK, file.data.into_owned()).into_response();
    let headers = response.headers_mut();
    if let Ok(mime) = HeaderValue::from_str(&mime) {
        headers.insert(header::CONTENT_TYPE, mime);
    }
    // The page's address may carry the token.
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

/// The router of a pending first-run setup.
pub fn router(state: Arc<SetupState>) -> Router {
    Router::new()
        .route("/api/setup/session", get(session).post(exchange_token))
        .fallback(gate)
        .with_state(state)
}

/// Serves `router` on `listener` until `shutdown` completes.
///
/// # Errors
/// If serving fails.
pub async fn serve(
    listener: tokio::net::TcpListener,
    router: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}
