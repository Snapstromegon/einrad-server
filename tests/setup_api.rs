//! First-run setup API: status, test mail and completion (ADR 0011).

mod common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use common::{TempDir, in_memory_migrated};
use db_sqlite::SqliteRepo;
use einrad_server::{
    config::{CliArgs, ConfigFile, Settings},
    setup::{SetupEnv, SetupState, TOKEN_FILE_NAME, pending_reason, router},
};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
use tower::ServiceExt;

const PASSWORD: &str = "Sicheres-Passwort1";
const SMTP_PASSWORD: &str = "smtp-password-that-must-not-leak";

struct Harness {
    dir: TempDir,
    db: SqlitePool,
    state: Arc<SetupState>,
    app: Router,
    cli: CliArgs,
}

impl Harness {
    fn config_path(&self) -> PathBuf {
        self.dir.0.join("config.toml")
    }

    fn token_file(&self) -> PathBuf {
        self.dir.0.join(TOKEN_FILE_NAME)
    }

    /// A request with the setup token.
    async fn call(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.call_with(method, uri, body, Some(self.state.token()))
            .await
    }

    async fn call_with(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
        token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            request = request.header("x-setup-token", token);
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
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn status(&self) -> Value {
        let (status, body) = self.call(Method::GET, "/api/setup/status", None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn complete(&self, body: Value) -> (StatusCode, Value) {
        self.call(Method::POST, "/api/setup/complete", Some(body))
            .await
    }

    async fn admins(&self) -> Vec<(String, String, bool)> {
        sqlx::query_as(
            "SELECT users.email, users.password, users.email_verified FROM users \
             JOIN user_roles ON user_roles.user_id = users.id WHERE role = 'admin'",
        )
        .fetch_all(&self.db)
        .await
        .unwrap()
    }
}

async fn harness(cli: CliArgs, config: Option<&str>) -> Harness {
    let dir = TempDir::new();
    let path = dir.0.join("config.toml");
    if let Some(config) = config {
        std::fs::write(&path, config).unwrap();
    }
    // Completion hands over to normal mode, which reads the data path.
    let data = dir.0.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let cli = CliArgs {
        config: Some(path),
        data: Some(data),
        local_player: None,
        ..cli
    };
    let db = in_memory_migrated().await;
    let state = SetupState::new(None, SetupEnv::new(db.clone(), cli.clone()));
    state.write_token_file().unwrap();
    Harness {
        app: router(state.clone()),
        dir,
        db,
        state,
        cli,
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

fn complete_body() -> Value {
    json!({
        "base_url": "https://cup.example.org",
        "smtp": {
            "server": "smtp://localhost:1025",
            "user": "cup@example.org",
            "password": SMTP_PASSWORD,
        },
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

// Status

#[tokio::test]
async fn status_lists_each_step_as_missing_env_or_done() {
    let cli = CliArgs {
        smtp_server: Some("smtp.example.org".into()),
        smtp_username: Some("cup@example.org".into()),
        smtp_password: Some(SMTP_PASSWORD.into()),
        ..Default::default()
    };
    let config = "[server]\nbase_url = \"https://cup.example.org\"\nreload_db_token = \"file-reload-token\"\n";
    let h = harness(cli, Some(config)).await;
    let status = h.status().await;
    let steps = &status["steps"];
    assert_eq!(steps["base_url"]["state"], "done");
    assert_eq!(steps["smtp"]["state"], "env");
    assert_eq!(steps["secrets"]["state"], "missing");
    assert_eq!(steps["secrets"]["fields"]["jwt_secret"], "missing");
    assert_eq!(steps["secrets"]["fields"]["reload_db_token"], "done");
    assert_eq!(steps["secrets"]["fields"]["metrics_token"], "missing");
    assert_eq!(steps["admin"]["state"], "missing");
    assert_eq!(status["writes_config"], true);
    assert_eq!(status["config"], "exists_incomplete");
}

#[tokio::test]
async fn status_never_contains_a_secret_value() {
    let cli = CliArgs {
        smtp_password: Some(SMTP_PASSWORD.into()),
        ..Default::default()
    };
    let config = "[auth]\njwt_secret = \"file-jwt-secret-value\"\n[server]\nreload_db_token = \"file-reload-token\"\nmetrics_token = \"file-metrics-token\"\n[smtp]\nserver = \"smtp.example.org\"\n";
    let h = harness(cli, Some(config)).await;
    let body = h.status().await.to_string();
    for secret in [
        SMTP_PASSWORD,
        "file-jwt-secret-value",
        "file-reload-token",
        "file-metrics-token",
    ] {
        assert!(!body.contains(secret), "{body}");
    }
}

#[tokio::test]
async fn status_with_a_complete_config_needs_no_file() {
    let h = harness(env_system_config(), None).await;
    let status = h.status().await;
    assert_eq!(status["writes_config"], false);
    assert_eq!(status["config"], "not_needed");
    assert_eq!(status["steps"]["admin"]["state"], "missing");
}

#[tokio::test]
async fn every_setup_endpoint_needs_the_setup_token() {
    let h = harness(CliArgs::default(), None).await;
    for (method, uri) in [
        (Method::GET, "/api/setup/status"),
        (Method::POST, "/api/setup/test-mail"),
        (Method::POST, "/api/setup/complete"),
    ] {
        let (status, _) = h
            .call_with(method.clone(), uri, Some(json!({})), None)
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        let (status, _) = h
            .call_with(method.clone(), uri, Some(json!({})), Some("wrong"))
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
    assert!(h.admins().await.is_empty());
    assert!(!h.config_path().exists());
}

// Test mail

#[tokio::test]
async fn the_test_mail_goes_through_the_given_settings() {
    let (url, mails) = fake_smtp().await;
    let h = harness(CliArgs::default(), None).await;
    let (status, body) = h
        .call(
            Method::POST,
            "/api/setup/test-mail",
            Some(json!({
                "smtp": { "server": url, "user": "cup@example.org", "password": "x" },
                "to": "max@example.org",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true, "{body}");
    let mails = mails.lock().unwrap();
    assert_eq!(mails.len(), 1);
    assert!(mails[0].contains("To: max@example.org"), "{}", mails[0]);
    assert!(mails[0].contains("From: cup@example.org"), "{}", mails[0]);
}

#[tokio::test]
async fn a_test_mail_uses_what_the_environment_already_sets() {
    let (url, mails) = fake_smtp().await;
    let cli = CliArgs {
        smtp_server: Some(url),
        smtp_username: Some("env@example.org".into()),
        smtp_password: Some("x".into()),
        ..Default::default()
    };
    let h = harness(cli, None).await;
    // The input can't redirect settings the environment sets.
    let (_, body) = h
        .call(
            Method::POST,
            "/api/setup/test-mail",
            Some(json!({
                "smtp": { "server": "smtp://127.0.0.1:1", "user": "other@example.org" },
                "to": "max@example.org",
            })),
        )
        .await;
    assert_eq!(body["ok"], true, "{body}");
    assert!(mails.lock().unwrap()[0].contains("From: env@example.org"));
}

#[tokio::test]
async fn a_failing_test_mail_returns_the_error() {
    let h = harness(CliArgs::default(), None).await;
    let (status, body) = h
        .call(
            Method::POST,
            "/api/setup/test-mail",
            Some(json!({
                "smtp": { "server": "smtp://127.0.0.1:1", "user": "cup@example.org", "password": "x" },
                "to": "max@example.org",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], false);
    assert!(!body["error"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn a_test_mail_to_a_bad_address_or_server_is_an_error_not_a_panic() {
    let h = harness(CliArgs::default(), None).await;
    for smtp in [
        json!({ "server": "smtp://", "user": "cup@example.org", "password": "x" }),
        json!({ "server": "not a host!", "user": "cup@example.org", "password": "x" }),
        json!({ "server": "smtp://127.0.0.1:1", "user": "no-address", "password": "x" }),
        json!({}),
    ] {
        let (status, body) = h
            .call(
                Method::POST,
                "/api/setup/test-mail",
                Some(json!({ "smtp": smtp, "to": "max@example.org" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], false, "{smtp}: {body}");
    }
}

// Completion

#[tokio::test]
async fn completion_writes_the_config_and_creates_the_admin() {
    let h = harness(CliArgs::default(), None).await;
    let mut body = complete_body();
    body["metrics"] = json!(true);
    let (status, done) = h.complete(body).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["config_written"], true);
    assert_eq!(done["restart_required"], false);
    let metrics_token = done["metrics_token"].as_str().unwrap().to_string();
    assert_eq!(metrics_token.len(), 43);

    // The file: private, with the generated secrets and the template.
    let path = h.config_path();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("# http_address = "), "{content}");
    assert!(content.contains("# data_path = "), "{content}");
    assert!(content.contains("# insecure_cookies = "), "{content}");
    assert!(content.contains("# local_player = "), "{content}");
    assert!(content.contains("# url = \"sqlite://"), "{content}");
    // Features are competition settings, not part of the system config.
    assert!(!content.contains("[features]"), "{content}");
    assert!(
        content.contains("created by the first-run setup and lives in the database"),
        "{content}"
    );
    assert!(content.contains(&metrics_token));
    let file = ConfigFile::parse(&content).unwrap();
    assert!(file.admin.is_none(), "{content}");
    assert!(!content.contains("[admin]"));
    assert_eq!(file.auth.jwt_secret.as_deref().map(str::len), Some(43));
    assert_eq!(
        file.server.reload_db_token.as_deref().map(str::len),
        Some(43)
    );
    assert_ne!(file.auth.jwt_secret, file.server.reload_db_token);
    assert_eq!(file.smtp.password.as_deref(), Some(SMTP_PASSWORD));

    // A restart with the file starts normally.
    let loaded = ConfigFile::load(Some(&path)).unwrap();
    let settings = Settings::resolve(CliArgs::default(), loaded);
    assert!(settings.is_ok());
    assert_eq!(pending_reason(&settings, &h.db).await.unwrap(), None);

    // The admin: hashed, verified.
    let admins = h.admins().await;
    assert_eq!(admins.len(), 1);
    let (email, hash, verified) = &admins[0];
    assert_eq!(email, "max@example.org");
    assert!(*verified);
    assert_ne!(hash, PASSWORD);
    assert!(password_auth::verify_password(PASSWORD, hash).is_ok());

    // The token file is gone, and every setup endpoint says it's done.
    assert!(!h.token_file().exists());
    for (method, uri) in [
        (Method::GET, "/api/setup/status"),
        (Method::POST, "/api/setup/test-mail"),
        (Method::POST, "/api/setup/complete"),
        (Method::GET, "/api/setup/session"),
        (Method::POST, "/api/setup/session"),
    ] {
        let (status, body) = h.call(method.clone(), uri, Some(json!({}))).await;
        assert_eq!(status, StatusCode::GONE, "{method} {uri}");
        assert_eq!(body["complete"], true);
    }
}

#[tokio::test]
async fn without_the_opt_in_there_is_no_metrics_token() {
    let h = harness(CliArgs::default(), None).await;
    let (_, done) = h.complete(complete_body()).await;
    assert!(done["metrics_token"].is_null());
    let content = std::fs::read_to_string(h.config_path()).unwrap();
    assert!(content.contains("# metrics_token"));
    assert!(
        ConfigFile::parse(&content)
            .unwrap()
            .server
            .metrics_token
            .is_none()
    );
}

#[tokio::test]
async fn values_set_by_the_environment_are_not_written() {
    let cli = CliArgs {
        smtp_server: Some("smtp://localhost:1025".into()),
        jwt_secret: Some("jwt-from-env".into()),
        base_url: Some("https://env.example.org".parse().unwrap()),
        ..Default::default()
    };
    let h = harness(cli, None).await;
    let mut body = complete_body();
    body["smtp"]["server"] = json!("smtp://other.example.org");
    body["base_url"] = json!("https://typed.example.org");
    let (status, done) = h.complete(body).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let content = std::fs::read_to_string(h.config_path()).unwrap();
    let file = ConfigFile::parse(&content).unwrap();
    assert!(file.smtp.server.is_none(), "{content}");
    assert!(file.auth.jwt_secret.is_none(), "{content}");
    assert!(file.server.base_url.is_none(), "{content}");
    assert_eq!(file.smtp.user.as_deref(), Some("cup@example.org"));
    assert!(file.server.reload_db_token.is_some());
    assert!(!content.contains("jwt-from-env"));
}

#[tokio::test]
async fn with_a_complete_config_only_the_admin_is_created() {
    let h = harness(env_system_config(), None).await;
    let body = json!({ "admin": admin_json() });
    let (status, done) = h.complete(body).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["config_written"], false);
    assert!(!h.config_path().exists());
    assert_eq!(h.admins().await.len(), 1);
    assert!(!h.token_file().exists());
}

#[tokio::test]
async fn an_existing_config_file_is_never_overwritten() {
    let h = harness(
        CliArgs::default(),
        Some("# mine\n[server]\nhttp_address = \"0.0.0.0:80\"\n"),
    )
    .await;
    let (status, body) = h.complete(complete_body()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["reason"], "exists");
    assert_eq!(body["partial"], true);
    assert_eq!(
        body["config_path"].as_str().map(Path::new),
        Some(h.config_path().as_path())
    );
    assert!(
        body["config_content"]
            .as_str()
            .unwrap()
            .contains("jwt_secret")
    );
    assert_eq!(
        std::fs::read_to_string(h.config_path()).unwrap(),
        "# mine\n[server]\nhttp_address = \"0.0.0.0:80\"\n"
    );
    assert!(h.admins().await.is_empty());
    assert!(h.token_file().exists());
    // No temp files are left behind.
    let names: Vec<_> = std::fs::read_dir(&h.dir.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
}

#[tokio::test]
async fn a_broken_config_file_is_reported_without_its_content() {
    let h = harness(
        CliArgs::default(),
        Some("[smtp]\npassword = \"leaky-secret\n"),
    )
    .await;
    let (status, body) = h.call(Method::GET, "/api/setup/status", None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!body.to_string().contains("leaky-secret"), "{body}");
}

#[tokio::test]
async fn stored_credentials_are_not_sent_to_a_server_from_the_input() {
    let (url, mails) = fake_smtp().await;
    let cli = CliArgs {
        smtp_username: Some("env@example.org".into()),
        smtp_password: Some("env-secret".into()),
        ..Default::default()
    };
    let h = harness(cli, None).await;
    let (_, body) = h
        .call(
            Method::POST,
            "/api/setup/test-mail",
            Some(json!({ "smtp": { "server": url }, "to": "max@example.org" })),
        )
        .await;
    // The user and password of the input are needed, the stored ones don't count.
    assert_eq!(body["ok"], false, "{body}");
    assert!(mails.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_unwritable_config_path_returns_the_content() {
    let dir = TempDir::new();
    let cli = CliArgs {
        config: Some(dir.0.join("missing-dir").join("config.toml")),
        ..Default::default()
    };
    let db = in_memory_migrated().await;
    let state = SetupState::new(None, SetupEnv::new(db.clone(), cli));
    let h = Harness {
        app: router(state.clone()),
        dir,
        db,
        state,
        cli: CliArgs::default(),
    };
    let (status, body) = h.complete(complete_body()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["reason"], "not_writable");
    assert!(body["config_content"].as_str().unwrap().contains("[auth]"));
    assert!(h.admins().await.is_empty());
}

#[tokio::test]
async fn invalid_input_writes_nothing() {
    let h = harness(CliArgs::default(), None).await;
    let mut cases = vec![];
    let mut short = complete_body();
    short["admin"]["password"] = json!("Ab1!");
    short["admin"]["password_repeat"] = json!("Ab1!");
    cases.push(short);
    let mut differ = complete_body();
    differ["admin"]["password_repeat"] = json!("Anderes-Passwort1");
    cases.push(differ);
    let mut email = complete_body();
    email["admin"]["email"] = json!("not-an-email");
    cases.push(email);
    let mut name = complete_body();
    name["admin"]["name"] = json!("  ");
    cases.push(name);
    let mut url = complete_body();
    url["base_url"] = json!("ftp://cup.example.org");
    cases.push(url);
    let mut no_smtp = complete_body();
    no_smtp["smtp"]["password"] = json!("");
    cases.push(no_smtp);
    let mut bad_server = complete_body();
    bad_server["smtp"]["server"] = json!("smtp://");
    cases.push(bad_server);
    let mut bad_user = complete_body();
    bad_user["smtp"]["user"] = json!("no-address");
    cases.push(bad_user);
    cases.push(json!({}));
    for body in cases {
        let (status, response) = h.complete(body.clone()).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}: {response}"
        );
        assert!(!response["errors"].as_array().unwrap().is_empty());
    }
    assert!(!h.config_path().exists());
    assert!(h.admins().await.is_empty());
    assert!(h.token_file().exists());
}

#[tokio::test]
async fn an_email_used_by_another_user_is_rejected_before_anything_is_written() {
    let h = harness(CliArgs::default(), None).await;
    sqlx::query("INSERT INTO users (id, email, email_verified, name, password) VALUES ($1, 'max@example.org', true, 'Erika Beispiel', 'x')")
        .bind(uuid::Uuid::now_v7())
        .execute(&h.db)
        .await
        .unwrap();
    let (status, body) = h.complete(complete_body()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(!h.config_path().exists());
}

#[tokio::test]
async fn two_concurrent_completions_create_one_admin_and_one_file() {
    let h = harness(CliArgs::default(), None).await;
    let (a, b) = tokio::join!(h.complete(complete_body()), h.complete(complete_body()));
    let mut statuses = [a.0, b.0];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::GONE]);
    assert_eq!(h.admins().await.len(), 1);
    assert!(h.config_path().exists());
}

#[tokio::test]
async fn after_a_partial_failure_only_the_admin_is_missing() {
    // The file was written (or saved by hand) but the admin isn't there.
    let h = harness(CliArgs::default(), None).await;
    let (_, done) = h.complete(complete_body()).await;
    assert_eq!(done["config_written"], true);
    sqlx::query("DELETE FROM user_roles")
        .execute(&h.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users")
        .execute(&h.db)
        .await
        .unwrap();

    // A new process: the file is read again.
    let state = SetupState::new(None, SetupEnv::new(h.db.clone(), h.cli.clone()));
    let again = Harness {
        app: router(state.clone()),
        state,
        dir: TempDir(h.dir.0.clone()),
        db: h.db.clone(),
        cli: h.cli.clone(),
    };
    std::mem::forget(h);
    let status = again.status().await;
    assert_eq!(status["steps"]["admin"]["state"], "missing");
    for step in ["base_url", "smtp", "secrets"] {
        assert_eq!(status["steps"][step]["state"], "done", "{step}");
    }
    assert_eq!(status["writes_config"], false);
    let (status, done) = again.complete(json!({ "admin": admin_json() })).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["config_written"], false);
    assert_eq!(again.admins().await.len(), 1);
    let _ = std::fs::remove_dir_all(&again.dir.0);
}

#[tokio::test]
async fn a_missing_admin_is_required_while_none_exists() {
    let h = harness(env_system_config(), None).await;
    let (status, _) = h.complete(json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn setup_responses_are_not_cached() {
    let h = harness(CliArgs::default(), None).await;
    let request = Request::builder()
        .uri("/api/setup/status")
        .header("x-setup-token", h.state.token())
        .body(Body::empty())
        .unwrap();
    let response = h.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
}

// Follow-ups of the setup gate

#[test]
fn a_config_path_that_doesnt_exist_yet_is_an_empty_config() {
    let dir = TempDir::new();
    let file = ConfigFile::load(Some(&dir.0.join("config.toml"))).unwrap();
    assert!(file.smtp.server.is_none());
}

#[test]
fn another_read_error_still_fails() {
    let dir = TempDir::new();
    // A directory can't be read as a file.
    let err = ConfigFile::load(Some(&dir.0)).unwrap_err();
    assert!(err.contains("Couldn't read config file"), "{err}");
}

#[tokio::test]
async fn a_broken_competition_file_fails_the_early_check() {
    let dir = TempDir::new();
    let broken = dir.0.join("competition.toml");
    std::fs::write(&broken, "[competition]\nname = \"Cup\"\n").unwrap();
    assert!(einrad_server::seed::check_file(&broken).await.is_err());

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/competition.toml");
    einrad_server::seed::check_file(&fixture).await.unwrap();
}

#[tokio::test]
async fn an_admin_from_the_config_is_inserted_even_without_the_rest() {
    let db = in_memory_migrated().await;
    let inserted = einrad_server::admin_user::insert_admin_user(
        &SqliteRepo::from_pool(db.clone()),
        "Max Müller",
        "max@example.org",
        PASSWORD,
    )
    .await
    .unwrap();
    assert!(inserted);
    assert!(
        einrad_server::admin_user::admin_exists(&SqliteRepo::from_pool(db.clone()))
            .await
            .unwrap()
    );
    let again = einrad_server::admin_user::insert_admin_user(
        &SqliteRepo::from_pool(db.clone()),
        "Erika Beispiel",
        "erika@example.org",
        PASSWORD,
    )
    .await
    .unwrap();
    assert!(!again);
}
