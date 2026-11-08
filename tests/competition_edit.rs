//! Integration tests for "Wettkampf bearbeiten": editing the competition in
//! the admin UI (ADR 0005).

mod common;

use axum::http::StatusCode;
use common::{TempDir, TestApp, TestResponse, rider_body};
use serde_json::{Value, json};

async fn settings(app: &TestApp, cookie: &str) -> Value {
    let response = app
        .get("/api/query/get_competition_settings", Some(cookie))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body
}

async fn edit(app: &TestApp, cookie: &str, competition: &Value, photos: &Value) -> TestResponse {
    app.post(
        "/api/command/edit_competition",
        json!({ "competition": competition, "photos": photos }),
        Some(cookie),
    )
    .await
}

fn message(response: &TestResponse) -> &str {
    response.body["message"].as_str().unwrap_or_default()
}

#[tokio::test]
async fn only_admins_see_and_edit_the_competition() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let competition = settings(&app, &admin).await["competition"].clone();
    for roles in [&["jury_lead"][..], &["treasurer"], &[]] {
        let user = app
            .create_user_with_roles(&format!("User {roles:?}"), roles)
            .await;
        let cookie = app.cookie_for(user);
        assert_eq!(
            app.get("/api/query/get_competition_settings", Some(&cookie))
                .await
                .status,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            edit(&app, &cookie, &competition, &Value::Null).await.status,
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn the_settings_show_the_photo_password_and_the_feature_defaults() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    common::set_photos(
        &app.db,
        Some(&einrad_server::photos::PhotosSettings {
            source: "https://cloud.example.org/s/AbC123".into(),
            user: None,
            password: Some("geheim".into()),
        }),
    )
    .await;
    let admin = app.create_admin().await;
    let body = settings(&app, &admin).await;
    assert_eq!(body["photos"]["password"], "geheim");
    assert_eq!(
        body["feature_defaults"],
        json!({ "jury_registration": true, "moderation_input": true })
    );
    assert_eq!(
        body["competition"]["features"],
        json!({ "jury_registration": null, "moderation_input": null })
    );
    assert_eq!(body["determining_day_locked"], true);
    assert_eq!(body["file_id_mismatch"], Value::Null);
}

#[tokio::test]
async fn edits_apply_right_away() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_jury_registration(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    app.add_rider(&owner, rider_body(club_id, "Anna", true, false))
        .await;
    let club = app.get("/api/query/get_club", Some(&owner)).await;
    assert_eq!(club.body["amount_due"]["total"], 15.0);

    let mut competition = settings(&app, &admin).await["competition"].clone();
    competition["fee_per_start"] = json!(20);
    competition["venue"] = json!("  ");
    competition["features"] = json!({ "jury_registration": true, "moderation_input": false });
    let photos = TempDir::new();
    let response = edit(
        &app,
        &admin,
        &competition,
        &json!({ "source": photos.0.display().to_string(), "user": "", "password": "" }),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body["warnings"], json!([]));

    let club = app.get("/api/query/get_club", Some(&owner)).await;
    assert_eq!(club.body["amount_due"]["start_fees"], 20.0);
    let status = app.get("/api/query/get_system_status", None).await.body;
    assert_eq!(status["jury_registration"], true);
    assert_eq!(status["moderation_input"], false);
    assert_eq!(status["competition"]["venue"], Value::Null);
    assert!(app.photos.configured());
    let body = settings(&app, &admin).await;
    assert_eq!(body["photos"]["user"], Value::Null);
    assert_eq!(body["photos"]["password"], Value::Null);

    // No source, no photos.
    let response = edit(&app, &admin, &competition, &Value::Null).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert!(!app.photos.configured());
}

#[tokio::test]
async fn invalid_values_are_rejected_like_in_the_file() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let competition = settings(&app, &admin).await["competition"].clone();
    let before = competition.clone();

    let mut late = competition.clone();
    late["registration_end"] = json!("2030-01-01T00:00:00+01:00");
    let response = edit(&app, &admin, &late, &Value::Null).await;
    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        message(&response).contains("registration_end must not be after start_date"),
        "{:?}",
        response.body
    );

    let mut nameless = competition.clone();
    nameless["name"] = json!(" ");
    let response = edit(&app, &admin, &nameless, &Value::Null).await;
    assert!(
        message(&response).contains("name must not be empty"),
        "{:?}",
        response.body
    );

    let response = edit(
        &app,
        &admin,
        &competition,
        &json!({ "source": "https://cloud.example.org/s/", "user": null, "password": null }),
    )
    .await;
    assert!(
        message(&response).contains("[competition.photos]"),
        "{:?}",
        response.body
    );

    assert_eq!(settings(&app, &admin).await["competition"], before);
}

#[tokio::test]
async fn an_invalid_iban_saves_with_a_warning() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let mut competition = settings(&app, &admin).await["competition"].clone();
    competition["payment_iban"] = json!("DE00");
    let response = edit(&app, &admin, &competition, &Value::Null).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert!(
        response.body["warnings"][0]
            .as_str()
            .unwrap()
            .contains("IBAN"),
        "{:?}",
        response.body
    );
}

#[tokio::test]
async fn the_determining_day_is_locked_once_categories_exist() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let mut competition = settings(&app, &admin).await["competition"].clone();
    competition["determining_day"] = json!("2027-01-01");
    let response = edit(&app, &admin, &competition, &Value::Null).await;
    assert!(
        message(&response).contains("Stichtag"),
        "{:?}",
        response.body
    );

    for sql in [
        "DELETE FROM timeplan",
        "DELETE FROM final_categories",
        "DELETE FROM categories",
    ] {
        sqlx::query(sql).execute(&app.db).await.unwrap();
    }
    let response = edit(&app, &admin, &competition, &Value::Null).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        settings(&app, &admin).await["competition"]["determining_day"],
        "2027-01-01"
    );
}

#[tokio::test]
async fn the_id_never_changes_and_a_differing_file_is_flagged() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::started_with_competition_file(db, "anderer-wettkampf").await;
    let admin = app.create_admin().await;
    let body = settings(&app, &admin).await;
    assert_eq!(body["file_id_mismatch"], "anderer-wettkampf");

    let mut competition = body["competition"].clone();
    let id = competition["id"].clone();
    competition["id"] = json!("neue-id");
    let response = edit(&app, &admin, &competition, &Value::Null).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(settings(&app, &admin).await["competition"]["id"], id);
}
