//! First-run setup: the in-process handover into normal mode (ADR 0011).
//!
//! The tests drive the one router the process serves (`setup::router`), over
//! an in-memory database, with config and data in a temp dir.

mod common;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use common::{TempDir, in_memory_migrated};
use einrad_server::{
    config::{CliArgs, ConfigFile},
    jwt::JWTClaims,
    setup::{SetupEnv, SetupState, router},
};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
use tower::ServiceExt;

const PASSWORD: &str = "Sicheres-Passwort1";

struct Server {
    dir: TempDir,
    db: SqlitePool,
    state: Arc<SetupState>,
    app: Router,
}

struct Reply {
    status: StatusCode,
    cookies: Vec<String>,
    body: Value,
}

impl Reply {
    /// The `name=value` pair of a cookie this response sets.
    fn cookie(&self, name: &str) -> Option<String> {
        self.cookies
            .iter()
            .find(|c| c.starts_with(&format!("{name}=")))
            .map(|c| c.split(';').next().unwrap().to_string())
    }
}

impl Server {
    async fn start(cli: CliArgs) -> Self {
        let dir = TempDir::new();
        let data = dir.0.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let cli = CliArgs {
            config: Some(dir.0.join("config.toml")),
            data: Some(data),
            local_player: None,
            ..cli
        };
        let db = in_memory_migrated().await;
        let state = SetupState::new(None, SetupEnv::new(db.clone(), cli));
        Self {
            app: router(state.clone()),
            dir,
            db,
            state,
        }
    }

    fn config_path(&self) -> PathBuf {
        self.dir.0.join("config.toml")
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
        cookie: Option<&str>,
        token: bool,
    ) -> Reply {
        let mut request = Request::builder().method(method).uri(uri);
        if token {
            request = request.header("x-setup-token", self.state.token());
        }
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let body = match body {
            Some(body) => {
                request = request.header("content-type", "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let cookies = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        Reply {
            status,
            cookies,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    async fn api(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
        cookie: Option<&str>,
    ) -> Reply {
        self.send(method, uri, body, cookie, false).await
    }

    async fn complete(&self, body: Value) -> Reply {
        self.send(Method::POST, "/api/setup/complete", Some(body), None, true)
            .await
    }

    async fn setup_status(&self) -> Reply {
        self.send(Method::GET, "/api/setup/status", None, None, true)
            .await
    }
}

fn admin_json() -> Value {
    json!({
        "name": "Max Müller",
        "email": "max@example.org",
        "password": PASSWORD,
        "password_repeat": PASSWORD,
    })
}

fn complete_body(smtp_server: &str) -> Value {
    json!({
        "base_url": "https://cup.example.org",
        "smtp": { "server": smtp_server, "user": "cup@example.org", "password": "smtp-pw" },
        "metrics": false,
        "admin": admin_json(),
    })
}

fn env_system_config() -> CliArgs {
    CliArgs {
        smtp_server: Some("smtp://localhost:1025".into()),
        smtp_username: Some("cup@example.org".into()),
        smtp_password: Some("from-env".into()),
        jwt_secret: Some("jwt-from-env".into()),
        ..Default::default()
    }
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/competition.toml")
}

/// Minimal SMTP server that accepts every mail and keeps its text.
async fn fake_smtp() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("smtp://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let mails = Arc::new(Mutex::new(Vec::new()));
    let seen = mails.clone();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            let seen = seen.clone();
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut lines = BufReader::new(read).lines();
                write.write_all(b"220 fake ESMTP\r\n").await.unwrap();
                let mut data = None::<String>;
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(mail) = data.as_mut() {
                        if line == "." {
                            seen.lock().unwrap().push(data.take().unwrap());
                            write.write_all(b"250 ok\r\n").await.unwrap();
                        } else {
                            mail.push_str(&line);
                            mail.push('\n');
                        }
                        continue;
                    }
                    let upper = line.to_uppercase();
                    let reply: &[u8] = if upper.starts_with("DATA") {
                        data = Some(String::new());
                        b"354 go ahead\r\n"
                    } else if upper.starts_with("QUIT") {
                        let _ = write.write_all(b"221 bye\r\n").await;
                        return;
                    } else {
                        b"250 ok\r\n"
                    };
                    write.write_all(reply).await.unwrap();
                }
            });
        }
    });
    (url, mails)
}

#[tokio::test]
async fn after_completion_the_normal_routes_answer_without_a_restart() {
    let s = Server::start(CliArgs::default()).await;
    // Before: the gate.
    let before = s
        .api(Method::GET, "/api/query/get_system_status", None, None)
        .await;
    assert_eq!(before.status, StatusCode::SERVICE_UNAVAILABLE);

    let done = s.complete(complete_body("smtp://localhost:1025")).await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);

    let after = s
        .api(Method::GET, "/api/query/get_system_status", None, None)
        .await;
    assert_eq!(after.status, StatusCode::OK, "{}", after.body);
    // No competition yet: the system is in setup mode.
    assert!(after.body["competition"].is_null(), "{}", after.body);
}

#[tokio::test]
async fn the_completion_logs_the_admin_in_with_a_cookie_of_the_new_secret() {
    let s = Server::start(CliArgs::default()).await;
    let done = s.complete(complete_body("smtp://localhost:1025")).await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    assert_eq!(done.body["restart_required"], false);
    assert_eq!(done.body["redirect"], "/admin-competition");

    let cookie = done.cookie("jwt").expect("a session cookie");
    let attributes = done.cookies.iter().find(|c| c.starts_with("jwt=")).unwrap();
    assert!(attributes.contains("HttpOnly"), "{attributes}");
    assert!(
        attributes.contains("Path=/;")
            || attributes.contains("Path=/,")
            || attributes.ends_with("Path=/"),
        "{attributes}"
    );
    let whoami = s
        .api(Method::GET, "/api/query/whoami", None, Some(&cookie))
        .await;
    assert_eq!(whoami.status, StatusCode::OK, "{}", whoami.body);
    assert_eq!(whoami.body["email"], "max@example.org");

    // It verifies with the secret the setup wrote into the file.
    let file = ConfigFile::load(Some(&s.config_path())).unwrap();
    let secret = file.auth.jwt_secret.unwrap();
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS512);
    validation.validate_aud = false;
    let token = cookie.strip_prefix("jwt=").unwrap();
    let claims = jsonwebtoken::decode::<JWTClaims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .expect("signed with the new secret")
    .claims;
    assert_eq!(claims.sub().to_string(), whoami.body["id"]);
}

#[tokio::test]
async fn the_admin_can_log_in_normally_after_the_switch() {
    let s = Server::start(CliArgs::default()).await;
    s.complete(complete_body("smtp://localhost:1025")).await;
    let login = s
        .api(
            Method::POST,
            "/api/command/login",
            Some(json!({ "email": "max@example.org", "password": PASSWORD })),
            None,
        )
        .await;
    assert_eq!(login.status, StatusCode::OK, "{}", login.body);
    assert!(login.cookie("jwt").is_some());
}

#[tokio::test]
async fn the_setup_cookie_ends_with_the_completion() {
    let s = Server::start(CliArgs::default()).await;
    let done = s.complete(complete_body("smtp://localhost:1025")).await;
    let ended = done
        .cookies
        .iter()
        .find(|c| c.starts_with("setup_session="))
        .expect("the setup cookie is cleared");
    assert!(ended.contains("Path=/api/setup"), "{ended}");
}

#[tokio::test]
async fn setup_endpoints_answer_already_done_afterwards() {
    let s = Server::start(CliArgs::default()).await;
    s.complete(complete_body("smtp://localhost:1025")).await;
    for (method, uri) in [
        (Method::GET, "/api/setup/status"),
        (Method::POST, "/api/setup/test-mail"),
        (Method::POST, "/api/setup/complete"),
    ] {
        let reply = s
            .send(method.clone(), uri, Some(json!({})), None, true)
            .await;
        assert_eq!(reply.status, StatusCode::GONE, "{method} {uri}");
        assert_eq!(reply.body["complete"], true);
        assert_eq!(reply.body["restart_required"], false);
    }
}

#[tokio::test]
async fn a_present_competition_file_is_imported_at_the_handover() {
    let s = Server::start(CliArgs {
        competition: Some(fixture()),
        ..Default::default()
    })
    .await;
    let done = s.complete(complete_body("smtp://localhost:1025")).await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    let status = s
        .api(Method::GET, "/api/query/get_system_status", None, None)
        .await;
    assert_eq!(status.body["competition"]["id"], "einrad-demo-2027");
}

#[tokio::test]
async fn only_the_admin_missing_still_hands_over() {
    let s = Server::start(env_system_config()).await;
    let done = s.complete(json!({ "admin": admin_json() })).await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    assert_eq!(done.body["config_written"], false);
    assert!(!s.config_path().exists());
    let cookie = done.cookie("jwt").unwrap();
    let whoami = s
        .api(Method::GET, "/api/query/whoami", None, Some(&cookie))
        .await;
    assert_eq!(whoami.status, StatusCode::OK, "{}", whoami.body);
    // The environment's secret signs the cookie.
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS512);
    validation.validate_aud = false;
    jsonwebtoken::decode::<JWTClaims>(
        cookie.strip_prefix("jwt=").unwrap(),
        &jsonwebtoken::DecodingKey::from_secret(b"jwt-from-env"),
        &validation,
    )
    .unwrap();
}

#[tokio::test]
async fn mails_after_the_switch_use_the_new_smtp_server_and_base_url() {
    let (url, mails) = fake_smtp().await;
    let s = Server::start(CliArgs::default()).await;
    let done = s.complete(complete_body(&url)).await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    let reset = s
        .api(
            Method::POST,
            "/api/command/request_password_reset",
            Some(json!({ "email": "max@example.org" })),
            None,
        )
        .await;
    assert_eq!(reset.status, StatusCode::OK, "{}", reset.body);
    let mails = mails.lock().unwrap();
    assert_eq!(mails.len(), 1);
    assert!(mails[0].contains("From: cup@example.org"), "{}", mails[0]);
    assert!(
        mails[0].contains("https://cup.example.org/reset_password?token="),
        "{}",
        mails[0]
    );
}

#[tokio::test]
async fn a_competition_file_that_fails_is_a_422_before_anything_is_saved() {
    // The competition file is read again at the handover; a broken one must
    // fail before the config file or the admin are written.
    let broken = TempDir::new();
    let path = broken.0.join("competition.toml");
    std::fs::write(&path, "this is not a competition").unwrap();
    let s = Server::start(CliArgs {
        competition: Some(path),
        ..Default::default()
    })
    .await;
    let done = s.complete(complete_body("smtp://localhost:1025")).await;
    assert_eq!(
        done.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        done.body
    );
    assert!(done.body["errors"].to_string().contains("competition"));
    assert!(!s.config_path().exists());
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&s.db)
        .await
        .unwrap();
    assert_eq!(users, 0);
    // The setup is still open.
    assert_eq!(s.setup_status().await.status, StatusCode::OK);
}

#[tokio::test]
async fn a_competition_file_that_fails_with_a_complete_config_is_a_422_too() {
    let broken = TempDir::new();
    let path = broken.0.join("competition.toml");
    std::fs::write(&path, "this is not a competition").unwrap();
    let s = Server::start(CliArgs {
        competition: Some(path),
        ..env_system_config()
    })
    .await;
    let done = s.complete(json!({ "admin": admin_json() })).await;
    assert_eq!(
        done.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        done.body
    );
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&s.db)
        .await
        .unwrap();
    assert_eq!(users, 0);
}

// Complete system config, only the admin missing

#[tokio::test]
async fn status_marks_unneeded_settings_as_not_needed_when_only_the_admin_is_missing() {
    let s = Server::start(env_system_config()).await;
    let status = s.setup_status().await.body;
    assert_eq!(status["writes_config"], false);
    assert_eq!(status["steps"]["base_url"]["state"], "not_needed");
    assert_eq!(
        status["steps"]["secrets"]["fields"]["reload_db_token"],
        "not_needed"
    );
    assert_eq!(
        status["steps"]["secrets"]["fields"]["metrics_token"],
        "not_needed"
    );
    assert_eq!(status["steps"]["admin"]["state"], "missing");
}

#[tokio::test]
async fn completion_rejects_inputs_that_are_not_needed() {
    let s = Server::start(env_system_config()).await;
    let reply = s
        .complete(json!({
            "base_url": "https://cup.example.org",
            "metrics": true,
            "admin": admin_json(),
        }))
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        reply.body
    );
    let errors = reply.body["errors"].to_string();
    assert!(errors.contains("base_url"), "{errors}");
    assert!(errors.contains("metrics"), "{errors}");
    // Nothing happened.
    let admins: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&s.db)
        .await
        .unwrap();
    assert_eq!(admins, 0);
}

#[tokio::test]
async fn the_switch_happens_on_the_same_listener() {
    let s = Server::start(CliArgs::default()).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = s.app.clone();
    let server = tokio::spawn(einrad_server::setup::serve(
        listener,
        app,
        std::future::pending(),
    ));
    let client = reqwest::Client::new();
    let url = format!("{base}/api/query/get_system_status");
    assert_eq!(client.get(&url).send().await.unwrap().status(), 503);
    let done = client
        .post(format!("{base}/api/setup/complete"))
        .header("x-setup-token", s.state.token())
        .json(&complete_body("smtp://localhost:1025"))
        .send()
        .await
        .unwrap();
    assert_eq!(done.status(), 200);
    assert_eq!(client.get(&url).send().await.unwrap().status(), 200);
    server.abort();
}

#[tokio::test]
async fn completion_rejects_smtp_input_when_the_config_is_complete() {
    let s = Server::start(env_system_config()).await;
    let status = s.setup_status().await.body;
    // Set, so not actionable.
    assert_eq!(status["steps"]["smtp"]["state"], "env");
    let reply = s
        .complete(json!({
            "smtp": { "server": "smtp://localhost:1025", "user": "other@example.org", "password": "x" },
            "admin": admin_json(),
        }))
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        reply.body
    );
    let errors = reply.body["errors"].to_string();
    assert!(errors.contains("smtp"), "{errors}");
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&s.db)
        .await
        .unwrap();
    assert_eq!(users, 0);
}

const LEAKY_PASSWORD: &str = "hunter2-do-not-leak";

#[tokio::test]
async fn smtp_errors_never_echo_credentials() {
    let s = Server::start(CliArgs::default()).await;
    for server in [
        format!("smtp://max:{LEAKY_PASSWORD}@127.0.0.1:1"),
        format!("smtps://max:{LEAKY_PASSWORD}@"),
        format!("smtp://max:{LEAKY_PASSWORD}@exa mple.org"),
        format!("max:{LEAKY_PASSWORD}@not a host"),
    ] {
        // Test mail
        let mail = s
            .send(
                Method::POST,
                "/api/setup/test-mail",
                Some(json!({
                    "smtp": { "server": server, "user": "cup@example.org", "password": LEAKY_PASSWORD },
                    "to": "max@example.org",
                })),
                None,
                true,
            )
            .await;
        assert!(
            !mail.body.to_string().contains(LEAKY_PASSWORD),
            "{server}: {}",
            mail.body
        );
        // Completion
        let mut body = complete_body(&server);
        body["smtp"]["password"] = json!(LEAKY_PASSWORD);
        let done = s.complete(body).await;
        assert!(
            !done.body.to_string().contains(LEAKY_PASSWORD),
            "{server}: {}",
            done.body
        );
    }
}

// A stale setup token file

/// A normal start with a complete config and an admin, as the binary does it.
async fn normal_start(dir: &TempDir, db: &SqlitePool) {
    einrad_server::admin_user::insert_admin_user(db, "Max Müller", "max@example.org", PASSWORD)
        .await
        .unwrap();
    let cli = CliArgs {
        config: Some(dir.0.join("config.toml")),
        data: Some(dir.0.join("data")),
        local_player: None,
        ..env_system_config()
    };
    let settings = einrad_server::config::Settings::resolve(cli, ConfigFile::default()).unwrap();
    let report = einrad_server::seed::apply_settings(db, &settings)
        .await
        .unwrap();
    einrad_server::startup::build(db.clone(), settings, &report, &dir.0)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_normal_start_removes_a_stale_setup_token_file() {
    let dir = TempDir::new();
    let db = in_memory_migrated().await;
    let token_file = dir.0.join(einrad_server::setup::TOKEN_FILE_NAME);
    std::fs::write(&token_file, "stale-token\n").unwrap();
    normal_start(&dir, &db).await;
    assert!(!token_file.exists());
}

#[tokio::test]
async fn a_normal_start_without_a_token_file_is_fine() {
    let dir = TempDir::new();
    let db = in_memory_migrated().await;
    normal_start(&dir, &db).await;
}

#[tokio::test]
async fn a_start_in_first_run_setup_still_writes_a_fresh_token_file() {
    let s = Server::start(CliArgs::default()).await;
    s.state
        .announce(&einrad_server::setup::PendingReason::NoAdmin);
    let content =
        std::fs::read_to_string(s.dir.0.join(einrad_server::setup::TOKEN_FILE_NAME)).unwrap();
    assert_eq!(content.trim(), s.state.token());
}
