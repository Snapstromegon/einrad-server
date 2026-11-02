//! Integration tests for exporting the competition file and applying a
//! remap file of dropped judge registrations (ADR 0005).

mod common;

use axum::http::{StatusCode, header};
use common::{TestApp, rider_body};
use nrw_freestyle_cup_registration::{config::CompetitionFile, photos::PhotosSettings};
use serde_json::{Value, json};

const COMPETITION: &str = include_str!("fixtures/competition.toml");

async fn export(app: &TestApp, cookie: &str) -> String {
    let response = app.get("/api/query/export_competition", Some(cookie)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert!(
        response.headers[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .contains("einrad-demo-2027.toml")
    );
    response.body.as_str().unwrap().to_string()
}

async fn import(app: &TestApp, cookie: &str, file: &str) -> Value {
    let response = app
        .post(
            "/api/command/import_competition",
            json!({ "file": file }),
            Some(cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body
}

#[tokio::test]
async fn an_export_imports_into_a_fresh_database_and_exports_the_same() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV Exportverein").await;
    app.add_rider(&owner, rider_body(club_id, "Henrike", true, false))
        .await;
    common::set_photos(
        &app.db,
        Some(&PhotosSettings {
            source: "https://cloud.example.org/s/AbC123".into(),
            user: None,
            password: Some("geheim".into()),
        }),
    )
    .await;
    common::set_jury_registration(&app.db, Some(false)).await;
    // What only the admin timeplan creates: a labelled start block with its
    // own Einfahrzeit, an internal entry.
    for body in [
        json!({ "kind": "acts", "label": "Nachstarter", "einfahrzeit_seconds": 300 }),
        json!({ "label": "Jurybesprechung", "duration_seconds": 900, "internal": true, "show_photos": false }),
    ] {
        let response = app
            .post("/api/command/add_timeplan_entry", body, Some(&admin))
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    }

    let file = export(&app, &admin).await;
    assert!(!file.contains("RSV Exportverein"), "{file}");
    assert!(!file.contains("Henrike"), "{file}");
    assert!(file.contains("password = \"geheim\""), "{file}");
    assert!(file.contains("jury_registration = false"), "{file}");
    assert!(file.contains("kind = \"acts\""), "{file}");
    assert!(file.contains("einfahrzeit_minutes = 5.0"), "{file}");
    assert!(file.contains("internal = true"), "{file}");
    assert!(file.contains("registration_end = "), "{file}");
    CompetitionFile::parse(&file).unwrap();

    let fresh = TestApp::without_competition(common::in_memory_migrated().await).await;
    let fresh_admin = fresh.create_admin().await;
    assert_eq!(
        import(&fresh, &fresh_admin, &file).await["outcome"],
        "created"
    );
    assert_eq!(export(&fresh, &fresh_admin).await, file);

    // The exported file changes nothing in the database it came from.
    assert_eq!(import(&app, &admin, &file).await["outcome"], "unchanged");
}

#[tokio::test]
async fn only_admins_export() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    for roles in [&["jury_lead"][..], &["treasurer"], &[]] {
        let user = app
            .create_user_with_roles(&format!("User {roles:?}"), roles)
            .await;
        let cookie = app.cookie_for(user);
        assert_eq!(
            app.get("/api/query/export_competition", Some(&cookie))
                .await
                .status,
            StatusCode::FORBIDDEN
        );
    }
}

/// A replace that drops Max Müller's registration for "Kleingruppe
/// Expert"; returns the remap file's content.
async fn replace_dropping_a_registration(app: &TestApp, admin: &str) -> String {
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let categories = app
        .get("/api/query/list_jury_categories", Some(admin))
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
    let file = COMPETITION.replacen(
        "[[jury_categories]]\nname = \"Kleingruppe Expert\"",
        "[[jury_categories]]\nname = \"Kleingruppe Expert (neu)\"",
        1,
    );
    let body = import(app, admin, &file).await;
    body["remap_file"]["content"].as_str().unwrap().to_string()
}

async fn registrations(app: &TestApp) -> Vec<(String, String, String)> {
    sqlx::query_as(
        r#"
        SELECT judge.lastname, jury_categories.name, judge_registration.task
        FROM judge_registration
        JOIN judge ON judge.id = judge_registration.judge_id
        JOIN jury_categories ON jury_categories.id = judge_registration.jury_category_id
        "#,
    )
    .fetch_all(&app.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn a_filled_in_remap_file_registers_the_judges_again() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let remap = replace_dropping_a_registration(&app, &admin).await;
    assert_eq!(registrations(&app).await, vec![]);

    let apply = |file: String| {
        let app = &app;
        let admin = &admin;
        async move {
            let response = app
                .post(
                    "/api/command/apply_jury_remap",
                    json!({ "file": file }),
                    Some(admin),
                )
                .await;
            assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
            response.body
        }
    };

    // Not filled in: nothing happens, the entry comes back.
    let result = apply(remap.clone()).await;
    assert_eq!(result["registered"], 0);
    assert_eq!(result["unfilled"].as_array().unwrap().len(), 1);

    let unknown = remap.replace("jury_category = \"\"", "jury_category = \"Gibt es nicht\"");
    let result = apply(unknown).await;
    assert_eq!(result["not_found"][0]["problem"], "jury_category_not_found");

    let stranger = remap
        .replace(
            "jury_category = \"\"",
            "jury_category = \"Kleingruppe Expert (neu)\"",
        )
        .replace("Müller", "Unbekannt");
    let result = apply(stranger).await;
    assert_eq!(result["not_found"][0]["problem"], "judge_not_found");
    assert_eq!(
        result["not_found"][0]["registration"]["lastname"],
        "Unbekannt"
    );

    let filled = remap.replace(
        "jury_category = \"\"",
        "jury_category = \"Kleingruppe Expert (neu)\"",
    );
    let result = apply(filled.clone()).await;
    assert_eq!(result["registered"], 1, "{result:?}");
    assert_eq!(result["not_found"], json!([]));
    assert_eq!(
        registrations(&app).await,
        vec![(
            "Müller".to_string(),
            "Kleingruppe Expert (neu)".to_string(),
            "technical".to_string()
        )]
    );
    // Applying it twice changes nothing.
    assert_eq!(apply(filled).await["registered"], 1);
    assert_eq!(registrations(&app).await.len(), 1);
}

#[tokio::test]
async fn only_the_jury_lead_or_admins_apply_a_remap_file() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let user = app.create_user_with_roles("Kasse", &["treasurer"]).await;
    let response = app
        .post(
            "/api/command/apply_jury_remap",
            json!({ "file": "jury_registrations = []" }),
            Some(&app.cookie_for(user)),
        )
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
}
