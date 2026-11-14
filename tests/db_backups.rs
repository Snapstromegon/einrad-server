//! Integration tests for the admin's database backups (#353).

mod common;

use axum::http::{StatusCode, header};
use common::{TempDir, TestApp};
use serde_json::json;

const OLD: &str = "db.backup-2026-11-14T09-30-00.sqlite";
const NEW: &str = "db.backup-2026-11-15T08-00-00.sqlite";

async fn app(dir: &TempDir) -> TestApp {
    // A real file, as in production: VACUUM INTO needs one.
    let db = einrad_server::reloadable_sqlite::connect(&format!(
        "sqlite://{}?mode=rwc",
        dir.0.join("db.sqlite").display()
    ))
    .await
    .unwrap();
    db_sqlite::MIGRATOR.run(&db).await.unwrap();
    TestApp::with_db_dir(db, &dir.0).await
}

fn write(dir: &TempDir, name: &str, contents: &str) {
    std::fs::write(dir.0.join(name), contents).unwrap();
}

#[tokio::test]
async fn only_admins_can_use_the_backup_endpoints() {
    let dir = TempDir::new();
    let app = app(&dir).await;
    write(&dir, OLD, "backup");
    let treasurer = app.cookie_for(
        app.create_user_with_roles("Max Müller", &["treasurer"])
            .await,
    );
    let download = format!("/api/db-backups/{OLD}");

    for cookie in [Some(treasurer.as_str()), None] {
        let expected = if cookie.is_some() {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::UNAUTHORIZED
        };
        assert_eq!(
            app.get("/api/query/list_db_backups", cookie).await.status,
            expected
        );
        assert_eq!(
            app.post("/api/command/create_db_backup", json!({}), cookie)
                .await
                .status,
            expected
        );
        assert_eq!(
            app.post(
                "/api/command/delete_db_backups",
                json!({ "filenames": [OLD] }),
                cookie
            )
            .await
            .status,
            expected
        );
        let headers: Vec<_> = cookie.map(|c| (header::COOKIE, c)).into_iter().collect();
        assert_eq!(app.get_raw(&download, &headers).await.status, expected);
    }
    assert!(dir.0.join(OLD).exists());
}

#[tokio::test]
async fn create_list_download_and_delete_round_trip() {
    let dir = TempDir::new();
    let app = app(&dir).await;
    let admin = app.create_admin().await;
    write(&dir, NEW, "newer");
    write(&dir, OLD, "older");

    let list = app.get("/api/query/list_db_backups", Some(&admin)).await;
    assert_eq!(list.status, StatusCode::OK, "{:?}", list.body);
    let rows = list.body.as_array().unwrap();
    // Newest first.
    assert_eq!(rows[0]["filename"], NEW);
    assert_eq!(rows[0]["created_at"], "2026-11-15T08:00:00");
    assert_eq!(rows[0]["size_bytes"], 5);
    assert_eq!(rows[1]["filename"], OLD);
    assert_eq!(rows.len(), 2);

    let created = app
        .post("/api/command/create_db_backup", json!({}), Some(&admin))
        .await;
    assert_eq!(created.status, StatusCode::OK, "{:?}", created.body);
    let name = created.body["filename"].as_str().unwrap().to_string();
    assert!(dir.0.join(&name).is_file(),);
    // The new backup is a real database holding the admin.
    let backup = sqlx::SqlitePool::connect(&format!("sqlite://{}", dir.0.join(&name).display()))
        .await
        .unwrap();
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&backup)
        .await
        .unwrap();
    assert_eq!(users, 1);
    let list = app.get("/api/query/list_db_backups", Some(&admin)).await;
    assert_eq!(list.body.as_array().unwrap().len(), 3);

    let cookie = [(header::COOKIE, admin.as_str())];
    let download = app
        .get_raw(&format!("/api/db-backups/{OLD}"), &cookie)
        .await;
    assert_eq!(download.status, StatusCode::OK);
    assert_eq!(download.bytes, b"older");
    assert!(
        download.headers[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .contains(OLD)
    );

    let deleted = app
        .post(
            "/api/command/delete_db_backups",
            json!({ "filenames": [OLD, NEW] }),
            Some(&admin),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::OK, "{:?}", deleted.body);
    assert!(!dir.0.join(OLD).exists());
    assert!(!dir.0.join(NEW).exists());
    assert!(dir.0.join(&name).exists());
    assert_eq!(
        app.get_raw(&format!("/api/db-backups/{OLD}"), &cookie)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn only_exact_backup_names_in_the_listing_are_reachable() {
    let dir = TempDir::new();
    let app = app(&dir).await;
    let admin = app.create_admin().await;
    write(&dir, "other.backup-2026-01-01T00-00-00.sqlite", "other");
    write(&dir, "db.backup-2026-1-1T0-0-0.sqlite", "sloppy");
    write(&dir, "secret.txt", "secret");
    let cookie = [(header::COOKIE, admin.as_str())];

    let bad = [
        "db.sqlite",
        "../db.sqlite",
        "..%2Fdb.sqlite",
        "%2Fetc%2Fpasswd",
        "other.backup-2026-01-01T00-00-00.sqlite",
        "db.backup-2026-1-1T0-0-0.sqlite",
        "secret.txt",
        // Well-formed but not in the directory.
        "db.backup-2030-01-01T00-00-00.sqlite",
        "db.backup-2026-11-14T09-30-00.sqlite/../db.sqlite",
    ];
    for name in bad {
        let response = app
            .get_raw(&format!("/api/db-backups/{name}"), &cookie)
            .await;
        assert!(
            response.status == StatusCode::NOT_FOUND || response.status == StatusCode::BAD_REQUEST,
            "{name}: {}",
            response.status
        );
        assert_ne!(response.status, StatusCode::OK, "{name}");
    }

    let names: Vec<_> = bad.iter().map(|n| n.replace("%2F", "/")).collect();
    let deleted = app
        .post(
            "/api/command/delete_db_backups",
            json!({ "filenames": names }),
            Some(&admin),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NOT_FOUND, "{:?}", deleted.body);
    for kept in ["db.sqlite", "secret.txt", "db.backup-2026-1-1T0-0-0.sqlite"] {
        assert!(dir.0.join(kept).exists(), "{kept}");
    }
}

#[tokio::test]
async fn a_delete_with_one_bad_name_deletes_nothing() {
    let dir = TempDir::new();
    let app = app(&dir).await;
    let admin = app.create_admin().await;
    write(&dir, OLD, "older");

    let response = app
        .post(
            "/api/command/delete_db_backups",
            json!({ "filenames": [OLD, "db.sqlite"] }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert!(dir.0.join(OLD).exists());
}

#[tokio::test]
async fn an_in_memory_database_has_no_backups() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let list = app.get("/api/query/list_db_backups", Some(&admin)).await;
    assert_eq!(list.status, StatusCode::CONFLICT, "{:?}", list.body);
}
