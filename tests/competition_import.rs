//! Integration tests for setup mode and importing the competition file in
//! the admin UI (ADR 0005).

mod common;

use axum::http::StatusCode;
use common::{TempDir, TestApp, TestResponse};
use einrad_server::competition::Competition;
use serde_json::{Value, json};

const COMPETITION: &str = include_str!("fixtures/competition.toml");

async fn preview(app: &TestApp, cookie: &str, file: &str) -> TestResponse {
    app.post(
        "/api/command/preview_competition_import",
        json!({ "file": file }),
        Some(cookie),
    )
    .await
}

async fn import(app: &TestApp, cookie: &str, file: &str) -> TestResponse {
    app.post(
        "/api/command/import_competition",
        json!({ "file": file }),
        Some(cookie),
    )
    .await
}

async fn system_status(app: &TestApp) -> Value {
    app.get("/api/query/get_system_status", None).await.body
}

#[tokio::test]
async fn in_setup_mode_the_import_creates_the_competition() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_competition(db).await;
    let admin = app.create_admin().await;
    assert_eq!(system_status(&app).await["competition"], Value::Null);

    let response = preview(&app, &admin, COMPETITION).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body["current_id"], Value::Null);
    assert_eq!(response.body["name"], "Einradevent 2027 (Demo)");
    assert_eq!(
        Competition::current(&app.db).await.unwrap(),
        None,
        "a preview changes nothing"
    );

    let response = import(&app, &admin, COMPETITION).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body["outcome"], "created");
    assert_eq!(response.body["remap_file"], Value::Null);
    assert_eq!(
        system_status(&app).await["competition"]["name"],
        "Einradevent 2027 (Demo)"
    );
}

#[tokio::test]
async fn only_admins_import() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_competition(db).await;
    for roles in [&["jury_lead"][..], &["treasurer"], &[]] {
        let user = app
            .create_user_with_roles(&format!("User {roles:?}"), roles)
            .await;
        let cookie = app.cookie_for(user);
        assert_eq!(
            preview(&app, &cookie, COMPETITION).await.status,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            import(&app, &cookie, COMPETITION).await.status,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(Competition::current(&app.db).await.unwrap(), None);
}

#[tokio::test]
async fn an_invalid_file_changes_nothing_and_says_why() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let before = Competition::current(&app.db).await.unwrap();

    let response = import(&app, &admin, "[competition\nid = ").await;
    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        response.body["message"]
            .as_str()
            .unwrap()
            .contains("Ungültige Wettkampfdatei"),
        "{:?}",
        response.body
    );

    let late = COMPETITION.replacen(
        "registration_end = ",
        "registration_end = 2030-01-01T00:00:00+01:00\n# was: ",
        1,
    );
    for response in [
        preview(&app, &admin, &late).await,
        import(&app, &admin, &late).await,
    ] {
        assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            response.body["message"]
                .as_str()
                .unwrap()
                .contains("registration_end must not be after start_date"),
            "{:?}",
            response.body
        );
    }
    assert_eq!(Competition::current(&app.db).await.unwrap(), before);
}

#[tokio::test]
async fn a_replace_is_previewed_and_offers_the_remap_file() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let units = app
        .get("/api/query/list_jury_units", Some(&owner))
        .await
        .body;
    let unit = units
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["name"] == "Finale EW")
        .unwrap()
        .clone();
    let response = app
        .post(
            "/api/command/add_club_judge",
            json!({
                "club_id": club_id,
                "firstname": "Max",
                "lastname": "Müller",
                "birthdate": "1997-01-01T00:00:00.000Z",
                "is_trained": true,
                "max_deployments": null,
                "note": null,
                "registrations": [{ "unit_kind": unit["kind"], "unit_id": unit["id"], "task": "technical" }],
            }),
            Some(&owner),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let before = Competition::current(&app.db).await.unwrap().unwrap();

    let file = COMPETITION
        .replacen("fee_per_start = 15", "fee_per_start = 20", 1)
        .replacen(
            "[[finals]]\nname = \"Finale EW\"",
            "[[finals]]\nname = \"Finale EW (neu)\"",
            1,
        );
    assert_ne!(
        file,
        COMPETITION.replacen("fee_per_start = 15", "fee_per_start = 20", 1)
    );
    let response = preview(&app, &admin, &file).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body["current_id"], before.id);
    assert_eq!(response.body["changes"], json!(["settings", "finals"]));
    let dropped = &response.body["dropped"]["judge_registrations"];
    assert_eq!(dropped.as_array().unwrap().len(), 1, "{dropped:?}");
    assert_eq!(dropped[0]["old_jury_unit"], "final: Finale EW");
    assert_eq!(
        Competition::current(&app.db).await.unwrap().unwrap(),
        before,
        "a preview changes nothing"
    );

    let response = import(&app, &admin, &file).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body["outcome"], "replaced");
    let remap = &response.body["remap_file"];
    assert!(
        remap["name"]
            .as_str()
            .unwrap()
            .starts_with("jury-registrations-"),
        "{remap:?}"
    );
    let content = remap["content"].as_str().unwrap();
    assert!(content.contains("Müller"), "{content}");
    assert!(content.contains("jury_unit = \"\""), "{content}");
    let after = Competition::current(&app.db).await.unwrap().unwrap();
    assert_eq!(after.fee_per_start, 20.0);
    assert_eq!(after.id, before.id);

    // The same file again changes nothing.
    let response = import(&app, &admin, &file).await;
    assert_eq!(response.body["outcome"], "unchanged");
}

#[tokio::test]
async fn imported_features_and_photos_apply_without_a_restart() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_competition(db).await;
    let admin = app.create_admin().await;
    let folder = TempDir::new();
    let file = format!(
        "{COMPETITION}\n[competition.features]\njury_registration = false\nmoderation_input = false\n\
         [competition.photos]\nsource = {:?}\n",
        folder.0.display().to_string()
    );
    assert!(!app.photos.configured());

    let response = import(&app, &admin, &file).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(system_status(&app).await["jury_registration"], false);
    assert_eq!(system_status(&app).await["moderation_input"], false);
    assert!(app.photos.configured());
}

// Jury sessions in the file (#386, ADR 0019).

/// The fixture with `sessions` (TOML, `[[heats.jury_sessions]]` tables) on
/// the heat JE EW (EWU11, EWU13, EWU15).
fn with_heat_sessions(sessions: &str) -> String {
    COMPETITION.replacen(
        "categories = [\"EWU11\", \"EWU13\", \"EWU15\"]\n",
        &format!("categories = [\"EWU11\", \"EWU13\", \"EWU15\"]\n{sessions}"),
        1,
    )
}

async fn unit_named(app: &TestApp, cookie: &str, name: &str) -> Value {
    let response = app.get("/api/query/list_jury_units", Some(cookie)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["name"] == name)
        .unwrap_or_else(|| panic!("no unit {name}"))
        .clone()
}

fn category_names(session: &Value) -> Vec<String> {
    session["categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect()
}

async fn count(db: &sqlx::SqlitePool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(db).await.unwrap()
}

#[tokio::test]
async fn a_file_with_jury_sessions_creates_them() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_competition(db).await;
    let admin = app.create_admin().await;
    let file = with_heat_sessions(
        "\n[[heats.jury_sessions]]\ncategories = [\"EWU11\", \"EWU13\"]\nneeded_technical = 3\n\
         \n[[heats.jury_sessions]]\ncategories = [\"EWU15\"]\n",
    );
    let response = import(&app, &admin, &file).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body["outcome"], "created");

    let unit = unit_named(&app, &admin, "JE EW").await;
    let sessions = unit["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2, "{unit}");
    assert_eq!(category_names(&sessions[0]), ["EWU11", "EWU13"]);
    assert_eq!(sessions[0]["needed_technical"], 3);
    assert_eq!(sessions[0]["override_technical"], 3);
    assert_eq!(category_names(&sessions[1]), ["EWU15"]);
    assert_eq!(sessions[1]["needed_technical"], unit["needed_technical"]);
    assert_eq!(sessions[1]["override_technical"], Value::Null);
    // The other units keep their one default session.
    let other = unit_named(&app, &admin, "E EW").await;
    assert_eq!(other["sessions"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn replace_recreates_the_sessions_from_the_file() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;

    // The jury lead splits JE EW and assigns a judge in the app.
    let unit = unit_named(&app, &admin, "JE EW").await;
    let first = unit["sessions"][0]["id"].clone();
    let categories = [
        app.category_id("EWU11").await,
        app.category_id("EWU13").await,
        app.category_id("EWU15").await,
    ];
    let response = app
        .post(
            "/api/command/set_jury_sessions",
            json!({ "kind": unit["kind"], "id": unit["id"], "sessions": [
                { "id": first, "category_ids": [categories[0]] },
                { "category_ids": [categories[1], categories[2]] },
            ] }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let response = app
        .post(
            "/api/command/add_club_judge",
            json!({
                "club_id": club_id, "firstname": "Max", "lastname": "Müller",
                "birthdate": "1997-01-01T00:00:00.000Z", "is_trained": true,
                "max_deployments": null, "note": null, "registrations": [],
            }),
            Some(&owner),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let response = app
        .post(
            "/api/command/assign_judge",
            json!({ "judge_id": response.body["judge_id"], "session_id": first, "task": "technical" }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        count(&app.db, "SELECT COUNT(*) FROM jury_assignments").await,
        1
    );

    // Another split replaces it: nothing else differs, so the sessions alone
    // make it a replace.
    let second = with_heat_sessions(
        "\n[[heats.jury_sessions]]\ncategories = [\"EWU11\", \"EWU13\"]\nneeded_dismounts = 2\n\
         \n[[heats.jury_sessions]]\ncategories = [\"EWU15\"]\n",
    );
    let response = import(&app, &admin, &second).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body["outcome"], "replaced");
    let unit = unit_named(&app, &admin, "JE EW").await;
    let sessions = unit["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2, "{unit}");
    assert_eq!(category_names(&sessions[0]), ["EWU11", "EWU13"]);
    assert_eq!(sessions[0]["override_dismounts"], 2);
    assert_eq!(category_names(&sessions[1]), ["EWU15"]);
    assert_eq!(
        count(&app.db, "SELECT COUNT(*) FROM jury_assignments").await,
        0
    );

    // One session per unit, two for JE EW, and none left over.
    let units = app
        .get("/api/query/list_jury_units", Some(&admin))
        .await
        .body;
    let expected: usize = units
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["sessions"].as_array().unwrap().len())
        .sum();
    assert_eq!(
        count(&app.db, "SELECT COUNT(*) FROM jury_sessions").await,
        expected as i64
    );
    assert_eq!(
        count(
            &app.db,
            "SELECT COUNT(*) FROM jury_session_categories WHERE session_id NOT IN (SELECT id FROM jury_sessions)"
        )
        .await,
        0
    );

    // The same file again changes nothing.
    let response = import(&app, &admin, &second).await;
    assert_eq!(response.body["outcome"], "unchanged");

    // A file that differs only in a session's override is a change too.
    let third = second.replacen("needed_dismounts = 2", "needed_dismounts = 1", 1);
    assert_ne!(third, second);
    let response = import(&app, &admin, &third).await;
    assert_eq!(response.body["outcome"], "replaced");
}
