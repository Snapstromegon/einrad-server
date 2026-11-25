//! Integration tests for exporting the competition file (ADR 0005).

mod common;

use axum::http::{StatusCode, header};
use common::{TestApp, rider_body};
use einrad_server::{config::CompetitionFile, photos::PhotosSettings};
use serde_json::{Value, json};

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

// Jury sessions in the file (#386, ADR 0019).

async fn units(app: &TestApp, cookie: &str) -> Vec<Value> {
    let response = app.get("/api/query/list_jury_units", Some(cookie)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_array().unwrap().clone()
}

async fn unit_named(app: &TestApp, cookie: &str, name: &str) -> Value {
    units(app, cookie)
        .await
        .into_iter()
        .find(|u| u["name"] == name)
        .unwrap_or_else(|| panic!("no unit {name}"))
}

/// Splits `unit` into new sessions with these categories.
async fn split(app: &TestApp, admin: &str, unit: &Value, groups: &[&[&str]]) {
    let mut sessions = vec![];
    for group in groups {
        let mut ids = vec![];
        for name in *group {
            ids.push(app.category_id(name).await);
        }
        sessions.push(json!({ "category_ids": ids }));
    }
    let response = app
        .post(
            "/api/command/set_jury_sessions",
            json!({ "kind": unit["kind"], "id": unit["id"], "sessions": sessions }),
            Some(admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

async fn override_needed(app: &TestApp, admin: &str, session: &Value, body: Value) {
    let mut body = body;
    body["id"] = session["id"].clone();
    let response = app
        .post("/api/command/edit_jury_session", body, Some(admin))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

/// Every unit's Durchgänge as (category names, overrides), by unit name.
async fn sessions_by_unit(
    app: &TestApp,
    cookie: &str,
) -> Vec<(String, Vec<(Vec<String>, [Value; 3])>)> {
    let mut all: Vec<_> = units(app, cookie)
        .await
        .into_iter()
        .map(|unit| {
            let mut sessions: Vec<_> = unit["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| {
                    let names = s["categories"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|c| c["name"].as_str().unwrap().to_string())
                        .collect();
                    (
                        names,
                        [
                            s["override_technical"].clone(),
                            s["override_performance"].clone(),
                            s["override_dismounts"].clone(),
                        ],
                    )
                })
                .collect();
            sessions.sort_by_key(|(names, _): &(Vec<String>, _)| names.clone());
            (unit["name"].as_str().unwrap().to_string(), sessions)
        })
        .collect();
    all.sort_by_key(|(name, _)| name.clone());
    all
}

#[tokio::test]
async fn export_writes_sessions_only_for_split_or_overridden_units() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;

    // Unsplit units without overrides: nothing in the file.
    let file = export(&app, &admin).await;
    assert!(!file.contains("jury_sessions"), "{file}");

    // One session with an override is written; the rest isn't.
    let e_ew = unit_named(&app, &admin, "E EW").await;
    override_needed(
        &app,
        &admin,
        &e_ew["sessions"][0],
        json!({ "needed_technical": 2 }),
    )
    .await;
    let parsed = CompetitionFile::parse(&export(&app, &admin).await).unwrap();
    let heat = |name: &str| parsed.heats.iter().find(|h| h.name == name).unwrap();
    assert_eq!(heat("E EW").jury_sessions.len(), 1);
    assert_eq!(heat("E EW").jury_sessions[0].categories, ["EWU17"]);
    assert_eq!(heat("E EW").jury_sessions[0].needed_technical, Some(2));
    assert_eq!(heat("E EW").jury_sessions[0].needed_performance, None);
    assert!(heat("JE EW").jury_sessions.is_empty());
    assert!(parsed.finals[0].jury_sessions.is_empty());

    // A split heat and a split final.
    let je_ew = unit_named(&app, &admin, "JE EW").await;
    split(&app, &admin, &je_ew, &[&["EWU11"], &["EWU13", "EWU15"]]).await;
    let finale = unit_named(&app, &admin, "Finale EW").await;
    split(
        &app,
        &admin,
        &finale,
        &[&["EWU11", "EWU13", "EWU15"], &["EWU17"]],
    )
    .await;
    let je_ew = unit_named(&app, &admin, "JE EW").await;
    override_needed(
        &app,
        &admin,
        &je_ew["sessions"][1],
        json!({ "needed_dismounts": 1 }),
    )
    .await;
    let parsed = CompetitionFile::parse(&export(&app, &admin).await).unwrap();
    let sessions = &parsed
        .heats
        .iter()
        .find(|h| h.name == "JE EW")
        .unwrap()
        .jury_sessions;
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].categories, ["EWU11"]);
    assert_eq!(sessions[0].needed_dismounts, None);
    assert_eq!(sessions[1].categories, ["EWU13", "EWU15"]);
    assert_eq!(sessions[1].needed_dismounts, Some(1));
    let sessions = &parsed.finals[0].jury_sessions;
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].categories, ["EWU11", "EWU13", "EWU15"]);
    assert_eq!(sessions[1].categories, ["EWU17"]);
    assert!(sessions.iter().all(|s| s.needed_technical.is_none()));
}

#[tokio::test]
async fn export_then_import_keeps_the_split() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let je_ew = unit_named(&app, &admin, "JE EW").await;
    split(&app, &admin, &je_ew, &[&["EWU11", "EWU13"], &["EWU15"]]).await;
    let je_ew = unit_named(&app, &admin, "JE EW").await;
    override_needed(
        &app,
        &admin,
        &je_ew["sessions"][0],
        json!({ "needed_technical": 3, "needed_dismounts": 0 }),
    )
    .await;
    let finale = unit_named(&app, &admin, "Finale EW").await;
    split(
        &app,
        &admin,
        &finale,
        &[&["EWU11", "EWU13"], &["EWU15", "EWU17"]],
    )
    .await;
    let before = sessions_by_unit(&app, &admin).await;
    let file = export(&app, &admin).await;

    let fresh = TestApp::without_competition(common::in_memory_migrated().await).await;
    let fresh_admin = fresh.create_admin().await;
    assert_eq!(
        import(&fresh, &fresh_admin, &file).await["outcome"],
        "created"
    );
    assert_eq!(sessions_by_unit(&fresh, &fresh_admin).await, before);
    // The overrides came along.
    let je_ew = unit_named(&fresh, &fresh_admin, "JE EW").await;
    let overridden: Vec<_> = je_ew["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["override_technical"] == 3)
        .collect();
    assert_eq!(overridden.len(), 1);
    assert_eq!(overridden[0]["override_dismounts"], 0);

    // And the file exports the same again.
    assert_eq!(export(&fresh, &fresh_admin).await, file);
}
