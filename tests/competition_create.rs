//! Integration tests for creating a competition from scratch in setup mode
//! (ADR 0005, ADR 0011).

mod common;

use axum::http::StatusCode;
use common::{TestApp, TestResponse};
use einrad_server::competition::Competition;
use serde_json::{Value, json};

fn body() -> Value {
    json!({
        "id": "testcup-2027",
        "name": "Testcup 2027",
        "location": "Musterstadt",
        "start_date": "2027-06-05",
        "end_date": "2027-06-06",
        "registration_start": "2026-12-01T00:00:00+01:00",
        "registration_end": "2027-04-30T23:59:59+02:00",
        "music_upload_end": "2027-05-15T23:59:59+02:00",
        "fee_per_start": 15.5,
    })
}

fn with(field: &str, value: Value) -> Value {
    let mut body = body();
    body[field] = value;
    body
}

async fn create(app: &TestApp, cookie: &str, body: Value) -> TestResponse {
    app.post("/api/command/create_competition", body, Some(cookie))
        .await
}

async fn setup_app() -> (TestApp, String) {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_competition(db).await;
    let admin = app.create_admin().await;
    (app, admin)
}

#[tokio::test]
async fn in_setup_mode_creating_makes_the_competition_and_leaves_setup_mode() {
    let (app, admin) = setup_app().await;
    let status = app.get("/api/query/get_system_status", None).await.body;
    assert_eq!(status["competition"], Value::Null);

    let response = create(&app, &admin, body()).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let competition = Competition::current(&app.db).await.unwrap().unwrap();
    assert_eq!(competition.id, "testcup-2027");
    assert_eq!(competition.name, "Testcup 2027");
    assert_eq!(competition.location, "Musterstadt");
    assert_eq!(competition.fee_per_start, 15.5);
    assert_eq!(competition.determining_day.to_string(), "2027-06-01");
    assert_eq!(competition.max_fee_per_rider, None);
    let status = app.get("/api/query/get_system_status", None).await.body;
    assert_eq!(status["competition"]["name"], "Testcup 2027");
}

#[tokio::test]
async fn only_admins_create() {
    let (app, _) = setup_app().await;
    for roles in [&["jury_lead"][..], &["treasurer"], &[]] {
        let user = app
            .create_user_with_roles(&format!("User {roles:?}"), roles)
            .await;
        let response = create(&app, &app.cookie_for(user), body()).await;
        assert_eq!(response.status, StatusCode::FORBIDDEN);
    }
    let response = app
        .post("/api/command/create_competition", body(), None)
        .await;
    assert_ne!(response.status, StatusCode::OK);
    assert_eq!(Competition::current(&app.db).await.unwrap(), None);
}

#[tokio::test]
async fn creating_is_refused_while_a_competition_exists() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let before = Competition::current(&app.db).await.unwrap();

    for id in ["testcup-2027", "einrad-demo-2027"] {
        let response = create(&app, &admin, with("id", json!(id))).await;
        assert_ne!(response.status, StatusCode::OK);
        assert!(
            response.body["message"]
                .as_str()
                .unwrap()
                .contains("schon einen Wettkampf"),
            "{:?}",
            response.body
        );
    }
    assert_eq!(Competition::current(&app.db).await.unwrap(), before);
}

#[tokio::test]
async fn invalid_fields_are_explained_in_german_and_create_nothing() {
    let (app, admin) = setup_app().await;
    let cases = [
        ("end_date", json!("2027-06-04"), "Das Ende"),
        ("name", json!("  "), "Der Name"),
        ("location", json!(""), "Der Ort"),
        ("fee_per_start", json!(-1), "Die Gebühr"),
        (
            "registration_end",
            json!("2027-07-01T00:00:00+02:00"),
            "Die Anmeldung",
        ),
        (
            "registration_end",
            json!("2026-11-01T00:00:00+01:00"),
            "Der Anmeldebeginn",
        ),
        (
            "music_upload_end",
            json!("2027-07-01T00:00:00+02:00"),
            "Der Musik-Upload",
        ),
        ("id", json!(""), "Die ID"),
        ("id", json!("Test Cup"), "Die ID"),
        ("id", json!("-testcup"), "Die ID"),
    ];
    for (field, value, message) in cases {
        let response = create(&app, &admin, with(field, value.clone())).await;
        assert_ne!(response.status, StatusCode::OK, "{field} {value}");
        assert!(
            response.body["message"].as_str().unwrap().contains(message),
            "{field} {value}: {:?}",
            response.body
        );
        assert_eq!(Competition::current(&app.db).await.unwrap(), None);
    }
    // All problems at once.
    let mut all = with("name", json!(""));
    all["location"] = json!("");
    let response = create(&app, &admin, all).await;
    let message = response.body["message"].as_str().unwrap();
    assert!(message.contains("Der Name") && message.contains("Der Ort"));
}
