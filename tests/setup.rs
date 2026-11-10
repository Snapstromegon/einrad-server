//! First-run setup: the trigger, the setup token and the gate.
//!
//! The gate serves the setup shell `static/setup.html`, so these tests need
//! the built frontend, like the server itself.

mod common;

use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc};

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use common::{TempDir, in_memory_migrated};
use einrad_server::{
    config::{CliArgs, ConfigFile, Settings},
    setup::{PendingReason, SetupState, TOKEN_FILE_NAME, config_dir, pending_reason, router},
};
use tower::ServiceExt;
use url::Url;

const COMPLETE_CONFIG: &str = r#"
    [smtp]
    server = "smtp.example.org"
    user = "cup@example.org"
    password = "secret"

    [auth]
    jwt_secret = "a-long-random-secret"
"#;

fn resolve(config: &str) -> Result<Settings, String> {
    Settings::resolve(CliArgs::default(), ConfigFile::parse(config).unwrap())
}

async fn add_admin(db: &sqlx::SqlitePool) {
    sqlx::query("INSERT INTO users (id, email, email_verified, name, password) VALUES (?, 'admin@cup.test', true, 'Max Müller', 'x')")
        .bind(uuid::Uuid::now_v7())
        .execute(db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_roles (user_id, role) SELECT id, 'admin' FROM users")
        .execute(db)
        .await
        .unwrap();
}

fn base_url() -> Option<Url> {
    Some(Url::parse("https://cup.example.org").unwrap())
}

fn state_in(dir: &TempDir, base_url: Option<Url>) -> Arc<SetupState> {
    SetupState::new(base_url, &dir.0)
}

async fn send(app: &Router, method: Method, uri: &str, body: Option<&str>) -> Response {
    let mut request = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(body) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_owned())
        }
        None => Body::empty(),
    };
    app.clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap()
}

async fn text(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn token_json(token: &str) -> String {
    serde_json::json!({ "token": token }).to_string()
}

fn setup_cookie(response: &Response) -> String {
    response
        .headers()
        .get(header::SET_COOKIE)
        .expect("a Set-Cookie header")
        .to_str()
        .unwrap()
        .to_owned()
}

// The trigger

#[tokio::test]
async fn missing_smtp_starts_the_setup() {
    let db = in_memory_migrated().await;
    add_admin(&db).await;
    let settings = resolve("[auth]\njwt_secret = \"a-long-random-secret\"");
    let reason = pending_reason(&settings, &db).await.unwrap().unwrap();
    match reason {
        PendingReason::IncompleteConfig(missing) => assert!(missing.contains("SMTP"), "{missing}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn missing_jwt_secret_starts_the_setup() {
    let db = in_memory_migrated().await;
    add_admin(&db).await;
    let settings = resolve("[smtp]\nserver = \"s\"\nuser = \"u\"\npassword = \"p\"");
    let reason = pending_reason(&settings, &db).await.unwrap().unwrap();
    match reason {
        PendingReason::IncompleteConfig(missing) => {
            assert!(missing.contains("JWT secret"), "{missing}");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn no_admin_starts_the_setup_even_with_a_complete_config() {
    let db = in_memory_migrated().await;
    let reason = pending_reason(&resolve(COMPLETE_CONFIG), &db)
        .await
        .unwrap();
    assert_eq!(reason, Some(PendingReason::NoAdmin));
}

#[tokio::test]
async fn a_complete_config_with_an_admin_starts_normally() {
    let db = in_memory_migrated().await;
    add_admin(&db).await;
    let reason = pending_reason(&resolve(COMPLETE_CONFIG), &db)
        .await
        .unwrap();
    assert_eq!(reason, None);
}

#[test]
fn a_config_with_competition_sections_is_an_error_not_a_setup() {
    let err = ConfigFile::parse("[competition]\nname = \"Cup\"").unwrap_err();
    assert!(err.contains("competition file"), "{err}");
    let err = ConfigFile::parse("this is not toml").unwrap_err();
    assert!(!err.is_empty());
}

#[test]
fn the_token_file_goes_next_to_the_config_path() {
    assert_eq!(
        config_dir(Some(Path::new("/etc/cup/config.toml"))),
        Path::new("/etc/cup")
    );
    assert_eq!(config_dir(Some(Path::new("config.toml"))), Path::new("."));
    assert_eq!(config_dir(None), Path::new("."));
}

// The token and its file

#[test]
fn the_token_file_is_private_and_holds_the_setup_url_with_a_base_url() {
    let dir = TempDir::new();
    let state = state_in(&dir, base_url());
    state.write_token_file().unwrap();

    let path = dir.0.join(TOKEN_FILE_NAME);
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        content.trim(),
        format!("https://cup.example.org/setup?token={}", state.token())
    );
}

#[test]
fn the_token_file_holds_only_the_bare_token_without_a_base_url() {
    let dir = TempDir::new();
    let state = state_in(&dir, None);
    state.write_token_file().unwrap();

    let content = std::fs::read_to_string(dir.0.join(TOKEN_FILE_NAME)).unwrap();
    assert_eq!(content.trim(), state.token());
    assert_eq!(state.token_line(), state.token());
}

#[test]
fn a_base_url_with_a_path_keeps_it() {
    let dir = TempDir::new();
    let url = Url::parse("https://example.org/cup/").unwrap();
    let state = state_in(&dir, Some(url));
    assert_eq!(
        state.token_line(),
        format!("https://example.org/cup/setup?token={}", state.token())
    );
}

#[test]
fn every_start_makes_a_new_token_and_replaces_the_file() {
    let dir = TempDir::new();
    let first = state_in(&dir, None);
    first.write_token_file().unwrap();
    // An older file with wider permissions is replaced, not kept as is.
    let path = dir.0.join(TOKEN_FILE_NAME);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    let second = state_in(&dir, None);
    second.write_token_file().unwrap();

    assert_ne!(first.token(), second.token());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap().trim(),
        second.token()
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!first.token_matches(second.token()));
    assert!(!second.token_matches(first.token()));
}

#[test]
fn an_unwritable_directory_is_an_error_not_a_panic() {
    let dir = TempDir::new();
    let state = SetupState::new(None, &dir.0.join("missing"));
    assert!(state.write_token_file().is_err());
    // The token is still known, for the log.
    assert!(!state.token().is_empty());
}

#[test]
fn the_token_matches_bare_or_inside_a_url() {
    let dir = TempDir::new();
    let state = state_in(&dir, base_url());
    let token = state.token().to_owned();
    assert!(state.token_matches(&token));
    assert!(state.token_matches(&format!("  {token}\n")));
    assert!(state.token_matches(&state.token_line()));
    assert!(state.token_matches(&format!("/setup?token={token}&x=1")));
    assert!(!state.token_matches("wrong"));
    assert!(!state.token_matches(""));
    assert!(!state.token_matches(&format!("{token}x")));
}

// The gate

#[tokio::test]
async fn pages_serve_the_setup_shell() {
    let dir = TempDir::new();
    let app = router(state_in(&dir, None));
    for path in [
        "/",
        "/setup?token=abc",
        "/index.html",
        "/admin/anything",
        "/setup.html",
    ] {
        let response = send(&app, Method::GET, path, None).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(
            response.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html"),
            "{path}"
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
        let body = text(response).await;
        assert!(body.contains("cup-setup"), "{path}: not the setup shell");
    }
    assert_eq!(
        send(&app, Method::HEAD, "/", None).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn every_other_api_answers_503() {
    let dir = TempDir::new();
    let app = router(state_in(&dir, None));
    let routes = [
        (Method::GET, "/api/query/competition"),
        (Method::POST, "/api/command/login"),
        (Method::POST, "/api/command/register"),
        (Method::POST, "/api/command/reload_db"),
        (Method::GET, "/api/ws/player"),
        (Method::GET, "/metrics"),
        (Method::GET, "/songs/a.mp3"),
        (Method::GET, "/openapi.json"),
        (Method::GET, "/swagger/"),
        (Method::GET, "/api/setup/unknown"),
        (Method::POST, "/"),
    ];
    for (method, path) in routes {
        let response = send(&app, method.clone(), path, Some("{}")).await;
        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "{method} {path}"
        );
        assert_eq!(text(response).await, "first-run setup pending");
    }
}

// The token exchange

#[tokio::test]
async fn the_setup_api_needs_the_token_or_its_cookie() {
    let dir = TempDir::new();
    let state = state_in(&dir, None);
    let app = router(state.clone());

    let response = send(&app, Method::GET, "/api/setup/session", None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let request = Request::builder()
        .uri("/api/setup/session")
        .header("x-setup-token", state.token())
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn the_bare_token_or_a_url_is_exchanged_for_an_http_only_cookie() {
    let dir = TempDir::new();
    let state = state_in(&dir, None);
    let app = router(state.clone());

    for input in [
        state.token().to_owned(),
        format!("https://cup.example.org/setup?token={}", state.token()),
    ] {
        let response = send(
            &app,
            Method::POST,
            "/api/setup/session",
            Some(&token_json(&input)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let cookie = setup_cookie(&response);
        assert!(cookie.contains("HttpOnly"), "{cookie}");
        assert!(cookie.contains("SameSite=Strict"), "{cookie}");
        assert!(cookie.contains("Max-Age=1800"), "{cookie}");
        assert!(
            !cookie.contains(state.token()),
            "the cookie must not hold the token: {cookie}"
        );

        let pair = cookie.split(';').next().unwrap().to_owned();
        let request = Request::builder()
            .uri("/api/setup/session")
            .header(header::COOKIE, pair)
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}

#[tokio::test]
async fn a_wrong_token_is_rejected_without_a_cookie() {
    let dir = TempDir::new();
    let app = router(state_in(&dir, None));
    let response = send(
        &app,
        Method::POST,
        "/api/setup/session",
        Some(&token_json("wrong")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().get(header::SET_COOKIE).is_none());
}

#[tokio::test]
async fn a_forged_cookie_does_not_unlock() {
    let dir = TempDir::new();
    let app = router(state_in(&dir, None));
    let request = Request::builder()
        .uri("/api/setup/session")
        .header(header::COOKIE, "setup_session=forged")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn the_token_stays_valid_after_an_exchange() {
    let dir = TempDir::new();
    let state = state_in(&dir, None);
    let app = router(state.clone());
    let body = token_json(state.token());
    for _ in 0..2 {
        let response = send(&app, Method::POST, "/api/setup/session", Some(&body)).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}

#[tokio::test]
async fn the_cookie_is_secure_for_an_https_base_url_only() {
    let dir = TempDir::new();
    for (url, secure) in [
        (base_url(), true),
        (None, false),
        (Some(Url::parse("http://10.0.0.2:3000").unwrap()), false),
    ] {
        let state = state_in(&dir, url);
        let app = router(state.clone());
        let response = send(
            &app,
            Method::POST,
            "/api/setup/session",
            Some(&token_json(state.token())),
        )
        .await;
        assert_eq!(setup_cookie(&response).contains("Secure"), secure);
    }
}
