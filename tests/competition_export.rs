//! Integration tests for exporting the competition file and applying a
//! remap file of dropped judge registrations (ADR 0005).

mod common;

use axum::http::{StatusCode, header};
use common::{TestApp, rider_body};
use einrad_server::{config::CompetitionFile, photos::PhotosSettings};
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
    // A heat whose Vorlauf is skipped.
    let response = app
        .post(
            "/api/command/set_heat_skip",
            json!({ "id": app.heat_id("E EW").await, "skip": true }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let file = export(&app, &admin).await;
    assert!(!file.contains("RSV Exportverein"), "{file}");
    assert!(!file.contains("Henrike"), "{file}");
    assert!(file.contains("password = \"geheim\""), "{file}");
    assert!(file.contains("jury_registration = false"), "{file}");
    assert!(file.contains("kind = \"acts\""), "{file}");
    assert!(file.contains("einfahrzeit_minutes = 5.0"), "{file}");
    assert!(file.contains("internal = true"), "{file}");
    assert!(file.contains("registration_end = "), "{file}");
    // Categories as ages, so the file carries over to the next season.
    assert!(file.contains("under_age = 11.0"), "{file}");
    assert!(file.contains("min_age = 70.0"), "{file}");
    assert!(!file.contains("birthday"), "{file}");
    // Heats with their categories; finals name them in their slot groups.
    assert!(file.contains("[[heats]]\nname = \"JE EW\""), "{file}");
    assert!(
        file.contains("categories = [\"EWU11\", \"EWU13\", \"EWU15\"]"),
        "{file}"
    );
    assert!(file.contains("heat = \"E EW\""), "{file}");
    // With their thresholds, and whether the Vorlauf is skipped.
    assert!(file.contains("threshold = 6"), "{file}");
    assert!(file.contains("threshold = 5\nskip = true"), "{file}");
    assert!(!file.contains("label = \"U"), "{file}");
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
async fn an_export_writes_name_and_code_and_refers_to_the_code() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let heat = app.heat_id("JE EW").await;
    let response = app
        .post(
            "/api/command/edit_heat",
            json!({
                "id": heat, "new_name": "Einzel weiblich – Junior Expert (U11–U15)",
                "description": null, "categories": ["EWU11", "EWU13", "EWU15"],
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let slot_groups = json!([
        { "heat_id": heat, "count": 2 },
        { "heat_id": app.heat_id("E EW").await, "count": 1 },
    ]);
    let response = app
        .post(
            "/api/command/edit_final",
            json!({
                "id": app.final_id("Finale EW").await, "new_name": "Finale Einzel weiblich",
                "description": null, "act_type": "single", "is_single_male": false,
                "einfahrzeit_seconds": 300, "act_duration_seconds": 180,
                "judge_duration_seconds": 120, "slot_groups": slot_groups,
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    // Planned, unless the fixture's timeplan has it already.
    if app.get("/api/query/list_finals", None).await.body[0]["timeplan_id"].is_null() {
        let response = app
            .post(
                "/api/command/add_timeplan_entry",
                json!({ "final_id": app.final_id("Finale EW").await }),
                Some(&admin),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    }

    let file = export(&app, &admin).await;
    assert!(
        file.contains(
            "[[heats]]\nname = \"Einzel weiblich – Junior Expert (U11–U15)\"\ncode = \"JE EW\""
        ),
        "{file}"
    );
    assert!(
        file.contains("[[finals]]\nname = \"Finale Einzel weiblich\"\ncode = \"Finale EW\""),
        "{file}"
    );
    // References use the code.
    assert!(file.contains("heat = \"JE EW\""), "{file}");
    assert!(file.contains("final = \"Finale EW\""), "{file}");
    CompetitionFile::parse(&file).unwrap();

    let fresh = TestApp::without_competition(common::in_memory_migrated().await).await;
    let fresh_admin = fresh.create_admin().await;
    import(&fresh, &fresh_admin, &file).await;
    assert_eq!(export(&fresh, &fresh_admin).await, file);
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

/// A replace that drops Max Müller's registration for the final "Finale
/// EW"; returns the remap file's content.
async fn replace_dropping_a_registration(app: &TestApp, admin: &str) -> String {
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let units = app
        .get("/api/query/list_jury_units", Some(admin))
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
    let file = COMPETITION.replacen(
        "[[finals]]\nname = \"Finale EW\"",
        "[[finals]]\nname = \"Finale EW (neu)\"",
        1,
    );
    let body = import(app, admin, &file).await;
    body["remap_file"]["content"].as_str().unwrap().to_string()
}

async fn registrations(app: &TestApp) -> Vec<(String, String, String)> {
    sqlx::query_as(
        r#"
        SELECT judge.lastname, view_jury_unit.name, judge_registration.task
        FROM judge_registration
        JOIN judge ON judge.id = judge_registration.judge_id
        JOIN view_jury_unit ON view_jury_unit.id = judge_registration.unit_id
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

    let unknown = remap.replace("jury_unit = \"\"", "jury_unit = \"final: Gibt es nicht\"");
    let result = apply(unknown).await;
    assert_eq!(result["not_found"][0]["problem"], "jury_unit_not_found");

    let stranger = remap
        .replace("jury_unit = \"\"", "jury_unit = \"final: Finale EW (neu)\"")
        .replace("Müller", "Unbekannt");
    let result = apply(stranger).await;
    assert_eq!(result["not_found"][0]["problem"], "judge_not_found");
    assert_eq!(
        result["not_found"][0]["registration"]["lastname"],
        "Unbekannt"
    );

    let filled = remap.replace("jury_unit = \"\"", "jury_unit = \"final: Finale EW (neu)\"");
    let result = apply(filled.clone()).await;
    assert_eq!(result["registered"], 1, "{result:?}");
    assert_eq!(result["not_found"], json!([]));
    assert_eq!(
        registrations(&app).await,
        vec![(
            "Müller".to_string(),
            "Finale EW (neu)".to_string(),
            "technical".to_string()
        )]
    );
    // Applying it twice changes nothing.
    assert_eq!(apply(filled).await["registered"], 1);
    assert_eq!(registrations(&app).await.len(), 1);
}

#[tokio::test]
async fn a_remap_file_does_not_register_for_a_jury_unit_that_takes_no_registrations() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let remap = replace_dropping_a_registration(&app, &admin).await;
    // D6: the jury lead switched registrations for the final off.
    let units = app
        .get("/api/query/list_jury_units", Some(&admin))
        .await
        .body;
    let unit = units
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["name"] == "Finale EW (neu)")
        .unwrap()
        .clone();
    let response = app
        .post(
            "/api/command/edit_jury_unit",
            json!({
                "kind": unit["kind"],
                "id": unit["id"],
                "registrable": false,
                "deployments": unit["deployments"],
                "needed_technical": unit["needed_technical"],
                "needed_performance": unit["needed_performance"],
                "needed_dismounts": unit["needed_dismounts"],
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let filled = remap.replace("jury_unit = \"\"", "jury_unit = \"final: Finale EW (neu)\"");
    let response = app
        .post(
            "/api/command/apply_jury_remap",
            json!({ "file": filled }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body["registered"], 0);
    assert_eq!(
        response.body["not_found"][0]["problem"],
        "jury_unit_not_registrable"
    );
    assert_eq!(registrations(&app).await, vec![]);
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
