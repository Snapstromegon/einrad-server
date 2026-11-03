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
    let categories = app
        .get("/api/query/list_jury_categories", Some(&owner))
        .await
        .body;
    let category = categories
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Kleingruppe Expert")
        .unwrap()["id"]
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
                "registrations": [{ "jury_category_id": category, "task": "technical" }],
            }),
            Some(&owner),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let before = Competition::current(&app.db).await.unwrap().unwrap();

    let file = COMPETITION
        .replacen("fee_per_start = 15", "fee_per_start = 20", 1)
        .replacen(
            "[[jury_categories]]\nname = \"Kleingruppe Expert\"",
            "[[jury_categories]]\nname = \"Kleingruppe Expert (neu)\"",
            1,
        );
    assert_ne!(
        file,
        COMPETITION.replacen("fee_per_start = 15", "fee_per_start = 20", 1)
    );
    let response = preview(&app, &admin, &file).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body["current_id"], before.id);
    assert_eq!(
        response.body["changes"],
        json!(["settings", "jury_categories"])
    );
    let dropped = &response.body["dropped"]["judge_registrations"];
    assert_eq!(dropped.as_array().unwrap().len(), 1, "{dropped:?}");
    assert_eq!(dropped[0]["old_jury_category"], "Kleingruppe Expert");
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
    assert!(content.contains("jury_category = \"\""), "{content}");
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
        "{COMPETITION}\n[competition.features]\njury_registration = false\n\
         [competition.photos]\nsource = {:?}\n",
        folder.0.display().to_string()
    );
    assert!(!app.photos.configured());

    let response = import(&app, &admin, &file).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(system_status(&app).await["jury_registration"], false);
    assert!(app.photos.configured());
}
