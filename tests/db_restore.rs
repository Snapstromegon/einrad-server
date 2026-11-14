//! Integration tests for restoring the database from a backup (#354).

mod common;

use std::path::Path;

use axum::http::StatusCode;
use common::{TempDir, TestApp};
use einrad_server::ids::ActId;
use einrad_server::player::{PlaybackState, PlayerKind, PlayerStatus};
use serde_json::{Value, json};
use sqlx::{Connection, SqliteConnection};
use uuid::Uuid;

async fn app(dir: &TempDir) -> TestApp {
    // A real file, as in production: VACUUM INTO and the swap need one.
    let db = einrad_server::reloadable_sqlite::connect(&format!(
        "sqlite://{}?mode=rwc",
        dir.0.join("db.sqlite").display()
    ))
    .await
    .unwrap();
    sqlx::migrate!("./migrations").run(&db).await.unwrap();
    TestApp::with_db_dir(db, &dir.0).await
}

/// A connection to a database file, for looking at it behind the server's back.
async fn open(path: &Path) -> SqliteConnection {
    SqliteConnection::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap()
}

async fn user_names(path: &Path) -> Vec<String> {
    let mut conn = open(path).await;
    let names = sqlx::query_scalar("SELECT name FROM users ORDER BY name")
        .fetch_all(&mut conn)
        .await
        .unwrap();
    conn.close().await.unwrap();
    names
}

async fn backup_now(app: &TestApp, admin: &str) -> String {
    let created = app
        .post("/api/command/create_db_backup", json!({}), Some(admin))
        .await;
    assert_eq!(created.status, StatusCode::OK, "{:?}", created.body);
    created.body["filename"].as_str().unwrap().to_string()
}

/// A copy of the database file under the next free backup name, for
/// tampering with.
fn copy_backup(dir: &TempDir, from: &str, stamp: &str) -> String {
    let name = format!("db.backup-{stamp}.sqlite");
    std::fs::copy(dir.0.join(from), dir.0.join(&name)).unwrap();
    name
}

async fn execute(path: &Path, sql: &'static str) {
    let mut conn = open(path).await;
    sqlx::query(sql).execute(&mut conn).await.unwrap();
    conn.close().await.unwrap();
}

fn backup_files(dir: &TempDir) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".backup-"))
        .collect();
    names.sort();
    names
}

async fn restore(app: &TestApp, admin: &str, filename: &str, force: bool) -> common::TestResponse {
    app.post(
        "/api/command/restore_db_backup",
        json!({ "filename": filename, "force": force }),
        Some(admin),
    )
    .await
}

#[tokio::test]
async fn restore_replaces_the_data_and_the_snapshot_undoes_it() {
    let dir = TempDir::new();
    let app = app(&dir).await;
    let admin = app.create_admin().await;
    let db = dir.0.join("db.sqlite");
    let backup = backup_now(&app, &admin).await;
    app.create_user("Erika Musterfrau", false).await;
    // The pool is the live one: the new user is in the live database.
    assert_eq!(user_names(&db).await, ["Admin", "Erika Musterfrau"]);
    let backup_bytes = std::fs::read(dir.0.join(&backup)).unwrap();

    let restored = restore(&app, &admin, &backup, false).await;

    assert_eq!(restored.status, StatusCode::OK, "{:?}", restored.body);
    assert_eq!(user_names(&db).await, ["Admin"]);
    // The server uses the restored database, and the login still works.
    let list = app.get("/api/query/list_db_backups", Some(&admin)).await;
    assert_eq!(list.status, StatusCode::OK, "{:?}", list.body);
    // The backup itself is untouched.
    assert_eq!(std::fs::read(dir.0.join(&backup)).unwrap(), backup_bytes);

    // The snapshot is in the list, and restoring it brings the user back.
    let snapshot = restored.body["snapshot_filename"].as_str().unwrap();
    assert!(
        list.body
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["filename"] == snapshot)
    );
    let undone = restore(&app, &admin, snapshot, false).await;
    assert_eq!(undone.status, StatusCode::OK, "{:?}", undone.body);
    assert_eq!(user_names(&db).await, ["Admin", "Erika Musterfrau"]);
}

#[tokio::test]
async fn corrupt_adminless_and_newer_backups_are_refused_even_with_force() {
    let dir = TempDir::new();
    let app = app(&dir).await;
    let admin = app.create_admin().await;
    let db = dir.0.join("db.sqlite");
    let good = backup_now(&app, &admin).await;
    app.create_user("Erika Musterfrau", false).await;

    let corrupt = "db.backup-2026-01-01T00-00-01.sqlite";
    std::fs::write(dir.0.join(corrupt), "this is not a database").unwrap();

    let adminless = copy_backup(&dir, &good, "2026-01-01T00-00-02");
    execute(&dir.0.join(&adminless), "DELETE FROM user_roles").await;

    let newer = copy_backup(&dir, &good, "2026-01-01T00-00-03");
    execute(
        &dir.0.join(&newer),
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (99991231000000, 'from_the_future', 1, x'00', 0)",
    )
    .await;

    let changed = copy_backup(&dir, &good, "2026-01-01T00-00-04");
    execute(
        &dir.0.join(&changed),
        "UPDATE _sqlx_migrations SET checksum = x'00' WHERE version = \
         (SELECT MIN(version) FROM _sqlx_migrations)",
    )
    .await;

    let files_before = backup_files(&dir);
    for name in [
        corrupt,
        adminless.as_str(),
        newer.as_str(),
        changed.as_str(),
    ] {
        let check = app
            .get(
                &format!("/api/query/check_db_backup?filename={name}"),
                Some(&admin),
            )
            .await;
        assert_eq!(check.status, StatusCode::OK, "{name}: {:?}", check.body);
        assert!(
            !check.body["blocking"].as_array().unwrap().is_empty(),
            "{name}: {:?}",
            check.body
        );
        let refused = restore(&app, &admin, name, true).await;
        assert_eq!(refused.status, StatusCode::CONFLICT, "{name}");
        assert_eq!(
            user_names(&db).await,
            ["Admin", "Erika Musterfrau"],
            "{name}"
        );
    }
    // Nothing happened: no snapshot either.
    assert_eq!(backup_files(&dir), files_before);
}

/// The database file at the schema before the latest migration, with an
/// Admin user, as an older release would have written it.
async fn old_schema_backup(dir: &TempDir, name: &str) -> String {
    let full = sqlx::migrate!("./migrations");
    let ups: Vec<_> = full
        .iter()
        .filter(|m| m.migration_type.is_up_migration())
        .cloned()
        .collect();
    let last = ups.last().unwrap().clone();
    let mut older = sqlx::migrate::Migrator::DEFAULT;
    older.migrations = ups[..ups.len() - 1].to_vec().into();
    let path = dir.0.join(name);
    let mut conn = SqliteConnection::connect(&format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .unwrap();
    older.run(&mut conn).await.unwrap();
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, email_verified, name, password) VALUES ($1, 'old@example.org', true, 'Alter Admin', 'x')")
        .bind(id)
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_roles (user_id, role) VALUES ($1, 'admin')")
        .bind(id)
        .execute(&mut conn)
        .await
        .unwrap();
    conn.close().await.unwrap();
    let _ = last;
    name.to_string()
}

#[tokio::test]
async fn an_older_backup_needs_force_and_is_migrated_without_another_backup() {
    let dir = TempDir::new();
    let app = app(&dir).await;
    let admin = app.create_admin().await;
    let db = dir.0.join("db.sqlite");
    let old = old_schema_backup(&dir, "db.backup-2026-01-01T00-00-01.sqlite").await;
    let old_bytes = std::fs::read(dir.0.join(&old)).unwrap();

    let check = app
        .get(
            &format!("/api/query/check_db_backup?filename={old}"),
            Some(&admin),
        )
        .await;
    assert_eq!(check.status, StatusCode::OK, "{:?}", check.body);
    assert_eq!(check.body["blocking"], json!([]));
    assert_eq!(
        check.body["pending_migrations"].as_array().unwrap().len(),
        1
    );

    let without = restore(&app, &admin, &old, false).await;
    assert_eq!(without.status, StatusCode::CONFLICT, "{:?}", without.body);
    assert!(
        without.body["message"]
            .as_str()
            .unwrap()
            .contains(check.body["pending_migrations"][0].as_str().unwrap()),
        "{:?}",
        without.body
    );
    assert_eq!(user_names(&db).await, ["Admin"]);

    let forced = restore(&app, &admin, &old, true).await;
    assert_eq!(forced.status, StatusCode::OK, "{:?}", forced.body);
    assert_eq!(user_names(&db).await, ["Alter Admin"]);
    let mut conn = open(&db).await;
    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(
        applied as usize,
        sqlx::migrate!("./migrations")
            .iter()
            .filter(|m| m.migration_type.is_up_migration())
            .count()
    );
    // The old backup and the pre-restore snapshot only: migrating the
    // restored database made no backup of its own.
    assert_eq!(backup_files(&dir).len(), 2, "{:?}", backup_files(&dir));
    assert_eq!(std::fs::read(dir.0.join(&old)).unwrap(), old_bytes);
}

#[tokio::test]
async fn nothing_is_restored_while_the_music_plays() {
    let dir = TempDir::new();
    let app = app(&dir).await;
    let admin = app.create_admin().await;
    let db = dir.0.join("db.sqlite");
    let backup = backup_now(&app, &admin).await;
    app.create_user("Erika Musterfrau", false).await;
    let player = app
        .player_hub
        .register(None, "Bühne".into(), PlayerKind::Web)
        .await;
    let status = |state| PlayerStatus {
        state,
        ..PlayerStatus::default()
    };
    app.player_hub
        .status(player.id, status(PlaybackState::Playing));

    let refused = restore(&app, &admin, &backup, false).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{:?}", refused.body);
    assert_eq!(refused.body["message"], "Wiedergabe zuerst stoppen.");
    assert_eq!(user_names(&db).await, ["Admin", "Erika Musterfrau"]);

    app.player_hub
        .status(player.id, status(PlaybackState::Idle));
    let restored = restore(&app, &admin, &backup, false).await;
    assert_eq!(restored.status, StatusCode::OK, "{:?}", restored.body);
}

#[tokio::test]
async fn the_player_state_is_reset_after_a_restore() {
    let dir = TempDir::new();
    let app = app(&dir).await;
    let admin = app.create_admin().await;
    let backup = backup_now(&app, &admin).await;
    let (_, mut updates) = app.player_hub.subscribe();
    app.player_hub
        .command(einrad_server::player::ControllerMessage::Volume { volume: 0.5 })
        .await
        .unwrap();
    while updates.try_recv().is_ok() {}

    let restored = restore(&app, &admin, &backup, false).await;

    assert_eq!(restored.status, StatusCode::OK, "{:?}", restored.body);
    // The fresh state was pushed to the controllers.
    assert!(updates.try_recv().is_ok());
}

#[tokio::test]
async fn only_admins_can_restore_check_and_manage_missing_songs() {
    let dir = TempDir::new();
    let app = app(&dir).await;
    let admin = app.create_admin().await;
    let backup = backup_now(&app, &admin).await;
    let treasurer = app.cookie_for(
        app.create_user_with_roles("Max Müller", &["treasurer"])
            .await,
    );
    let act = ActId::new();

    for cookie in [Some(treasurer.as_str()), None] {
        let expected = if cookie.is_some() {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::UNAUTHORIZED
        };
        assert_eq!(
            app.get(
                &format!("/api/query/check_db_backup?filename={backup}"),
                cookie
            )
            .await
            .status,
            expected
        );
        assert_eq!(
            app.get("/api/query/list_missing_songs", cookie)
                .await
                .status,
            expected
        );
        assert_eq!(
            app.post(
                "/api/command/restore_db_backup",
                json!({ "filename": backup, "force": true }),
                cookie
            )
            .await
            .status,
            expected
        );
        assert_eq!(
            app.post(
                "/api/command/clear_act_song",
                json!({ "act_id": act }),
                cookie
            )
            .await
            .status,
            expected
        );
    }
    // The reload token is no substitute for an admin.
    let with_token = app
        .post(
            &format!(
                "/api/command/restore_db_backup?token={}",
                common::RELOAD_DB_TOKEN
            ),
            json!({ "filename": backup, "force": true }),
            Some(&treasurer),
        )
        .await;
    assert_eq!(with_token.status, StatusCode::FORBIDDEN);
}

/// Sets the song columns of an act directly.
async fn set_song(app: &TestApp, act: ActId, file: &str, name: &str, normalized: Option<&str>) {
    sqlx::query(
        "UPDATE acts SET song_file = $1, song_file_name = $2, song_file_normalized = $3, \
         normalization_status = 'done' WHERE id = $4",
    )
    .bind(file)
    .bind(name)
    .bind(normalized)
    .bind(act)
    .execute(&app.db)
    .await
    .unwrap();
}

#[tokio::test]
async fn missing_songs_are_reported_and_missing_normalized_files_requeued() {
    let dir = TempDir::new();
    let app = app(&dir).await;
    let admin = app.create_admin().await;
    let (cookie, club) = app.create_club_owner("Max Müller", "Musterverein").await;
    let lost = app.add_single_act(&cookie, club, "Erika").await;
    let kept = app.add_single_act(&cookie, club, "Anna").await;
    set_song(&app, lost, &format!("{lost}.mp3"), "Verloren.mp3", None).await;
    std::fs::write(app.data_path().join(format!("{kept}.mp3")), b"song").unwrap();
    set_song(
        &app,
        kept,
        &format!("{kept}.mp3"),
        "Da.mp3",
        Some("old.normalized.mp3"),
    )
    .await;
    let backup = backup_now(&app, &admin).await;

    let check = app
        .get(
            &format!("/api/query/check_db_backup?filename={backup}"),
            Some(&admin),
        )
        .await;
    assert_eq!(check.status, StatusCode::OK, "{:?}", check.body);
    let missing = check.body["missing_songs"].as_array().unwrap();
    // Only the original counts as missing, not the normalized copy.
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert_eq!(missing[0]["act_id"], lost.to_string());
    assert_eq!(missing[0]["club"], "Musterverein");
    assert_eq!(missing[0]["song_file_name"], "Verloren.mp3");
    assert_eq!(missing[0]["song_file"], format!("{lost}.mp3"));

    let restored = restore(&app, &admin, &backup, false).await;
    assert_eq!(restored.status, StatusCode::OK, "{:?}", restored.body);
    let reported = restored.body["missing_songs"].as_array().unwrap();
    assert_eq!(reported.len(), 1);
    assert_eq!(reported[0]["act_id"], lost.to_string());
    // The kept song's normalized copy is queued again under a new name.
    let mut conn = open(&dir.0.join("db.sqlite")).await;
    let normalized: Option<String> =
        sqlx::query_scalar("SELECT song_file_normalized FROM acts WHERE id = $1")
            .bind(kept)
            .fetch_one(&mut conn)
            .await
            .unwrap();
    let normalized = normalized.unwrap();
    assert_ne!(normalized, "old.normalized.mp3");
    assert!(normalized.starts_with(&kept.to_string()));
    let listed = app.get("/api/query/list_missing_songs", Some(&admin)).await;
    assert_eq!(listed.body.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_missing_song_can_be_uploaded_again_or_marked_deleted() {
    let dir = TempDir::new();
    let app = app(&dir).await;
    let admin = app.create_admin().await;
    let (cookie, club) = app.create_club_owner("Max Müller", "Musterverein").await;
    let upload = app.add_single_act(&cookie, club, "Erika").await;
    let gone = app.add_single_act(&cookie, club, "Anna").await;
    for act in [upload, gone] {
        set_song(
            &app,
            act,
            &format!("{act}.mp3"),
            "Verloren.mp3",
            Some("n.mp3"),
        )
        .await;
        sqlx::query("UPDATE acts SET song_checked = TRUE WHERE id = $1")
            .bind(act)
            .execute(&app.db)
            .await
            .unwrap();
    }
    let listed = app.get("/api/query/list_missing_songs", Some(&admin)).await;
    assert_eq!(
        listed.body.as_array().unwrap().len(),
        2,
        "{:?}",
        listed.body
    );

    // An Admin uploads for an act of any club.
    let uploaded = app
        .post_file(
            &format!("/api/command/save_act_song?act_id={upload}"),
            "Neu.mp3",
            b"new song",
            Some(&admin),
        )
        .await;
    assert_eq!(uploaded.status, StatusCode::OK, "{:?}", uploaded.body);
    let cleared = app
        .post(
            "/api/command/clear_act_song",
            json!({ "act_id": gone }),
            Some(&admin),
        )
        .await;
    assert_eq!(cleared.status, StatusCode::OK, "{:?}", cleared.body);

    let listed = app.get("/api/query/list_missing_songs", Some(&admin)).await;
    assert_eq!(listed.body, json!([]));
    let act: Value = app
        .get(&format!("/api/query/get_act?act_id={gone}"), Some(&admin))
        .await
        .body;
    for field in ["song_file", "song_file_name", "song_file_normalized"] {
        assert_eq!(act[field], Value::Null, "{field}");
    }
    assert_eq!(act["song_checked"], false);
    let act: Value = app
        .get(&format!("/api/query/get_act?act_id={upload}"), Some(&admin))
        .await
        .body;
    assert_eq!(act["song_file_name"], "Neu.mp3");
    assert_eq!(act["song_checked"], false);

    let unknown = app
        .post(
            "/api/command/clear_act_song",
            json!({ "act_id": Uuid::now_v7() }),
            Some(&admin),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
}
