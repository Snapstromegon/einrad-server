//! Juries on heats and finals (#347, ADR 0016): the migration from jury
//! categories, and the jury units.

mod common;

const BEFORE_JURY_UNITS: i64 = 20261115120000;

async fn exec(db: &sqlx::SqlitePool, sql: &str) {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(db)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn texts(db: &sqlx::SqlitePool, sql: &'static str) -> Vec<String> {
    sqlx::query_scalar(sql).fetch_all(db).await.unwrap()
}

#[tokio::test]
async fn the_migration_maps_jury_categories_only_where_the_match_is_unambiguous() {
    let db = common::in_memory().await;
    let migrator = &db_sqlite::MIGRATOR;
    migrator.run_to(BEFORE_JURY_UNITS, &db).await.unwrap();
    exec(
        &db,
        "INSERT INTO heats (name) VALUES ('E EW'), ('JE EW'), ('Doppelt'), ('Doppelt Vorlauf')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO finals (name, act_type, \"order\") VALUES \
         ('Finale EW', 'single', 1), ('E EW', 'single', 2)",
    )
    .await;
    // 1 matches a heat by name with " Vorlauf" stripped, 2 a final exactly,
    // 3 a heat exactly; 4 names a heat and a final, 5 two heats, 6 nothing.
    exec(
        &db,
        "INSERT INTO jury_categories \
         (id, name, deployments, day, sort_order, needed_technical, needed_performance, needed_dismounts) VALUES \
         (x'01', 'JE EW Vorlauf', 3, '2027-06-05', 1, 4, 4, 2), \
         (x'02', 'Finale EW', 1, '2027-06-06', 2, 5, 5, 3), \
         (x'03', 'JE EW', 2, NULL, 3, 5, 5, 3), \
         (x'04', 'E EW', 2, NULL, 4, 5, 5, 3), \
         (x'05', 'Doppelt Vorlauf', 2, NULL, 5, 5, 5, 3), \
         (x'06', 'Einzel Expert Vorlauf', 2, NULL, 6, 5, 5, 3)",
    )
    .await;
    exec(
        &db,
        "INSERT INTO users (id, email, email_verified, name, password) \
         VALUES (x'11', 'owner@example.com', TRUE, 'Owner', 'x')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO clubs (id, name, owner_id) VALUES (x'10', 'RSV', x'11')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO judge (id, club_id, firstname, lastname, birthdate, is_trained) \
         VALUES (x'20', x'10', 'Max', 'Müller', '1990-01-01T00:00:00Z', TRUE)",
    )
    .await;
    exec(
        &db,
        "INSERT INTO judge_registration (judge_id, jury_category_id, task) VALUES \
         (x'20', x'01', 'technical'), (x'20', x'02', 'performance'), \
         (x'20', x'03', 'technical'), (x'20', x'04', 'dismounts'), \
         (x'20', x'05', 'technical'), (x'20', x'06', 'technical')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO jury_assignments (judge_id, jury_category_id, task) VALUES \
         (x'20', x'02', 'performance'), (x'20', x'06', 'technical')",
    )
    .await;

    migrator.run(&db).await.unwrap();

    // The heat got the deployments and needed judges of its category.
    assert_eq!(
        texts(
            &db,
            "SELECT name || ':' || jury_deployments || ':' || jury_needed_technical \
             FROM heats ORDER BY name"
        )
        .await,
        [
            "Doppelt:2:5",
            "Doppelt Vorlauf:2:5",
            "E EW:2:5",
            "JE EW:3:4"
        ]
    );
    // Only the unambiguous ones: 1 and 2, and 3 (an exact heat name, which
    // merges with 1 into the same unit and task).
    assert_eq!(
        texts(
            &db,
            "SELECT COALESCE((SELECT 'heat ' || name FROM heats WHERE id = heat_id), \
                (SELECT 'final ' || name FROM finals WHERE id = final_id)) || ' ' || task \
             FROM judge_registration ORDER BY 1"
        )
        .await,
        ["final Finale EW performance", "heat JE EW technical"]
    );
    // The rest is on the remap list.
    assert_eq!(
        texts(
            &db,
            "SELECT old_unit || ' ' || task FROM judge_registration_unmapped ORDER BY old_unit"
        )
        .await,
        [
            "Doppelt Vorlauf technical",
            "E EW dismounts",
            "Einzel Expert Vorlauf technical"
        ]
    );
    // The assignment of the matched final stays, the other one is dropped.
    assert_eq!(
        texts(
            &db,
            "SELECT (SELECT name FROM finals WHERE id = final_id) || ' ' || task FROM jury_assignments"
        )
        .await,
        ["Finale EW performance"]
    );
    // Every heat and final has an id.
    assert_eq!(
        texts(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM heats WHERE id IS NULL"
        )
        .await,
        ["0"]
    );

    // Going back keeps the unmapped registrations as categories of their
    // old name, next to the mapped ones.
    migrator.undo(&db, BEFORE_JURY_UNITS).await.unwrap();
    assert_eq!(
        texts(
            &db,
            "SELECT c.name || ' ' || r.task FROM judge_registration r \
             JOIN jury_categories c ON c.id = r.jury_category_id ORDER BY 1"
        )
        .await,
        [
            "Doppelt Vorlauf technical",
            "E EW dismounts",
            "Einzel Expert Vorlauf technical",
            "Finale EW performance",
            "JE EW Vorlauf technical"
        ]
    );
}

// ---------- Jury units over the API ----------

use axum::http::StatusCode;
use common::{TestApp, TestResponse};
use einrad_server::config::CompetitionFile;
use einrad_server::ids::ClubId;
use serde_json::{Value, json};
use uuid::Uuid;

const COMPETITION: &str = include_str!("fixtures/competition.toml");
/// Born so that the judge is 30 on the first competition day.
const ADULT: &str = "1997-01-01T00:00:00.000Z";

async fn units(app: &TestApp, cookie: &str) -> Vec<Value> {
    let response = app.get("/api/query/list_jury_units", Some(cookie)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_array().unwrap().clone()
}

async fn unit(app: &TestApp, cookie: &str, name: &str) -> Value {
    units(app, cookie)
        .await
        .into_iter()
        .find(|u| u["name"] == name)
        .unwrap_or_else(|| panic!("no jury unit {name}"))
}

fn judge_body(club_id: ClubId, firstname: &str, registrations: &[(&Value, &str)]) -> Value {
    json!({
        "club_id": club_id,
        "firstname": firstname,
        "lastname": "Jury",
        "birthdate": ADULT,
        "is_trained": true,
        "max_deployments": null,
        "note": null,
        "registrations": registrations
            .iter()
            .map(|(unit, task)| json!({ "unit_kind": unit["kind"], "unit_id": unit["id"], "task": task }))
            .collect::<Vec<_>>(),
    })
}

async fn add_judge(app: &TestApp, cookie: &str, body: Value) -> TestResponse {
    app.post("/api/command/add_club_judge", body, Some(cookie))
        .await
}

async fn judges(app: &TestApp, cookie: &str, club_id: ClubId) -> Vec<Value> {
    let response = app
        .get(
            &format!("/api/query/list_club_judges?club_id={club_id}"),
            Some(cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_array().unwrap().clone()
}

fn rejected(response: &TestResponse, message: &str) {
    assert!(
        !response.status.is_success(),
        "expected a rejection: {:?}",
        response.body
    );
    assert!(
        response.body.to_string().contains(message),
        "expected „{message}“ in {:?}",
        response.body
    );
}

#[test]
fn jury_categories_in_a_file_are_rejected_with_an_example() {
    let file = format!("{COMPETITION}\n[[jury_categories]]\nname = \"Block A\"\n");
    let error = CompetitionFile::parse(&file).unwrap_err();
    assert!(
        error.contains("[[jury_categories]] are gone")
            && error.contains("[[heats]]")
            && error.contains("jury_deployments"),
        "{error}"
    );
    assert!(CompetitionFile::parse(COMPETITION).is_ok());
}

#[tokio::test]
async fn the_file_sets_the_jury_settings_and_the_export_writes_the_ones_that_differ() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let file = COMPETITION
        .replacen(
            "name = \"E EW\"\n",
            "name = \"E EW\"\njury_deployments = 3\njury_registrable = false\n",
            1,
        )
        .replacen(
            "jury_needed_technical = 3",
            "jury_deployments = 2\njury_needed_technical = 3",
            1,
        );
    let response = app
        .post(
            "/api/command/import_competition",
            json!({ "file": file }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let heat = unit(&app, &admin, "E EW").await;
    assert_eq!(
        (heat["deployments"].clone(), heat["registrable"].clone()),
        (json!(3), json!(false))
    );
    assert_eq!(unit(&app, &admin, "Finale EW").await["deployments"], 2);
    // The default of a heat and of a final, when the file doesn't set one.
    assert_eq!(unit(&app, &admin, "JE EW").await["deployments"], 2);

    let exported = app.get("/api/query/export_competition", Some(&admin)).await;
    let content = exported.body.as_str().unwrap();
    assert!(content.contains("jury_deployments = 3"), "{content}");
    assert!(content.contains("jury_registrable = false"), "{content}");
    // Only what differs from the defaults: "JE EW" has none.
    assert_eq!(content.matches("jury_deployments").count(), 2, "{content}");
    // And the export imports again.
    assert!(CompetitionFile::parse(content).is_ok());
}

#[tokio::test]
async fn a_negative_jury_setting_in_a_file_is_rejected() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let file = COMPETITION.replacen(
        "name = \"E EW\"\n",
        "name = \"E EW\"\njury_deployments = -1\n",
        1,
    );
    let response = app
        .post(
            "/api/command/import_competition",
            json!({ "file": file }),
            Some(&admin),
        )
        .await;
    rejected(&response, "Heat E EW");
}

#[tokio::test]
async fn nobody_registers_for_a_unit_that_takes_no_registrations() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let heat = unit(&app, &admin, "JE EW").await;

    // A skipped Vorlauf (JURY-4, D6).
    let skipped = app
        .post(
            "/api/command/set_heat_skip",
            json!({ "id": heat["id"], "skip": true }),
            Some(&admin),
        )
        .await;
    assert_eq!(skipped.status, StatusCode::OK, "{:?}", skipped.body);
    let skipped_unit = unit(&app, &admin, "JE EW").await;
    assert_eq!(skipped_unit["registrable"], false);
    assert_eq!(skipped_unit["skipped"], true);
    assert_eq!(skipped_unit["registrable_setting"], true);
    let response = add_judge(
        &app,
        &owner,
        judge_body(club_id, "Anna", &[(&heat, "technical")]),
    )
    .await;
    rejected(&response, "kann niemand mehr gemeldet werden");

    // Un-skipping opens it again.
    app.post(
        "/api/command/set_heat_skip",
        json!({ "id": heat["id"], "skip": false }),
        Some(&admin),
    )
    .await;
    let response = add_judge(
        &app,
        &owner,
        judge_body(club_id, "Anna", &[(&heat, "technical")]),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    // Switched off by the jury lead, too.
    let final_unit = unit(&app, &admin, "Finale EW").await;
    app.post(
        "/api/command/edit_jury_unit",
        json!({
            "kind": "final", "id": final_unit["id"], "registrable": false, "deployments": 1,
            "needed_technical": 5, "needed_performance": 5, "needed_dismounts": 3,
        }),
        Some(&admin),
    )
    .await;
    let response = add_judge(
        &app,
        &owner,
        judge_body(club_id, "Berta", &[(&final_unit, "technical")]),
    )
    .await;
    rejected(&response, "kann niemand mehr gemeldet werden");
}

#[tokio::test]
async fn a_heat_skipped_after_registrations_keeps_counting_but_is_flagged() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let heat = unit(&app, &admin, "JE EW").await;
    let other = unit(&app, &admin, "E EW").await;
    let anna = add_judge(
        &app,
        &owner,
        judge_body(
            club_id,
            "Anna",
            &[(&heat, "technical"), (&other, "technical")],
        ),
    )
    .await;
    assert_eq!(anna.status, StatusCode::OK, "{:?}", anna.body);
    let judge_id = anna.body["judge_id"].clone();

    app.post(
        "/api/command/set_heat_skip",
        json!({ "id": heat["id"], "skip": true }),
        Some(&admin),
    )
    .await;
    let anna = judges(&app, &owner, club_id).await[0].clone();
    // Still 2 + 2 deployments (D7), the skipped one flagged.
    assert_eq!(anna["registered_deployments"], 4);
    assert_eq!(anna["counted_deployments"], 4);
    let flags: Vec<Value> = anna["registrations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| json!([r["unit_id"], r["flagged"]]))
        .collect();
    assert_eq!(
        flags,
        vec![json!([other["id"], false]), json!([heat["id"], true])]
    );

    // The club can still change other things about the judge; the flagged
    // registration may stay.
    let mut edit = judge_body(
        club_id,
        "Anna",
        &[(&heat, "technical"), (&other, "technical")],
    );
    edit.as_object_mut().unwrap().remove("club_id");
    edit["judge_id"] = judge_id.clone();
    edit["note"] = json!("Nur Samstag");
    let edited = app
        .post("/api/command/edit_club_judge", edit, Some(&owner))
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{:?}", edited.body);

    // The jury lead moves it (D8); the judge already has the other unit, so
    // the registrations merge.
    let moved = app
        .post(
            "/api/command/move_judge_registrations",
            json!({
                "judge_id": judge_id,
                "from_kind": "heat", "from_id": heat["id"],
                "to_kind": "heat", "to_id": other["id"],
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(moved.status, StatusCode::OK, "{:?}", moved.body);
    let anna = judges(&app, &owner, club_id).await[0].clone();
    assert_eq!(anna["registrations"].as_array().unwrap().len(), 1);
    assert_eq!(anna["registered_deployments"], 2);

    // Not for clubs, and not to a unit that doesn't exist.
    let body = json!({
        "judge_id": judge_id,
        "from_kind": "heat", "from_id": other["id"],
        "to_kind": "heat", "to_id": heat["id"],
    });
    let forbidden = app
        .post(
            "/api/command/move_judge_registrations",
            body.clone(),
            Some(&owner),
        )
        .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    let mut unknown = body;
    unknown["to_id"] = json!(Uuid::new_v4());
    let unknown = app
        .post(
            "/api/command/move_judge_registrations",
            unknown,
            Some(&admin),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn renaming_keeps_the_registrations_and_deleting_is_refused_while_there_are_any() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let heat = unit(&app, &admin, "E EW").await;
    let final_unit = unit(&app, &admin, "Finale EW").await;
    let added = add_judge(
        &app,
        &owner,
        judge_body(
            club_id,
            "Anna",
            &[(&heat, "technical"), (&final_unit, "performance")],
        ),
    )
    .await;
    assert_eq!(added.status, StatusCode::OK, "{:?}", added.body);

    // D3: the unit is its heat's or final's id, so a rename keeps it.
    let renamed = app
        .post(
            "/api/command/edit_heat",
            json!({ "id": heat["id"], "new_name": "Expert EW", "description": null, "categories": ["EWU17"] }),
            Some(&admin),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{:?}", renamed.body);
    let after = unit(&app, &admin, "Expert EW").await;
    assert_eq!(after["id"], heat["id"]);
    assert_eq!(
        judges(&app, &owner, club_id).await[0]["registrations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // D4.
    let refused = app
        .post(
            "/api/command/delete_final",
            json!({ "id": final_unit["id"] }),
            Some(&admin),
        )
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{:?}", refused.body);
    rejected(&refused, "JurorInnen gemeldet");
    // A heat that feeds no final: the jury refuses, with its own message.
    let added_heat = app
        .post(
            "/api/command/add_heat",
            json!({ "name": "Solo", "description": null, "categories": [] }),
            Some(&admin),
        )
        .await;
    assert_eq!(added_heat.status, StatusCode::OK, "{:?}", added_heat.body);
    let solo = unit(&app, &admin, "Solo").await;
    let second = add_judge(
        &app,
        &owner,
        judge_body(club_id, "Berta", &[(&solo, "technical")]),
    )
    .await;
    assert_eq!(second.status, StatusCode::OK, "{:?}", second.body);
    let refused = app
        .post(
            "/api/command/delete_heat",
            json!({ "id": solo["id"] }),
            Some(&admin),
        )
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{:?}", refused.body);
    rejected(&refused, "JurorInnen gemeldet");
    app.post(
        "/api/command/delete_club_judge",
        json!({ "judge_id": second.body["judge_id"] }),
        Some(&admin),
    )
    .await;
    let deleted = app
        .post(
            "/api/command/delete_heat",
            json!({ "id": solo["id"] }),
            Some(&admin),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::OK, "{:?}", deleted.body);

    // Without registrations the final can be deleted.
    app.post(
        "/api/command/delete_club_judge",
        json!({ "judge_id": added.body["judge_id"] }),
        Some(&admin),
    )
    .await;
    let deleted = app
        .post(
            "/api/command/delete_final",
            json!({ "id": final_unit["id"] }),
            Some(&admin),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::OK, "{:?}", deleted.body);
    assert!(
        units(&app, &admin)
            .await
            .iter()
            .all(|u| u["name"] != "Finale EW")
    );
}

#[tokio::test]
async fn unmapped_registrations_are_resolved_by_the_jury_lead() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let heat = unit(&app, &admin, "JE EW").await;
    let added = add_judge(&app, &owner, judge_body(club_id, "Anna", &[])).await;
    let judge_id = added.body["judge_id"].clone();
    // What the migration leaves of a jury category it could not map.
    for task in ["technical", "dismounts"] {
        sqlx::query(
            "INSERT INTO judge_registration_unmapped (judge_id, old_unit, task) VALUES ($1, 'Einzel Vorlauf Expert', $2)",
        )
        .bind(Uuid::parse_str(judge_id.as_str().unwrap()).unwrap())
        .bind(task)
        .execute(&app.db)
        .await
        .unwrap();
    }
    let anna = judges(&app, &owner, club_id).await[0].clone();
    let unmapped = anna["unmapped_registrations"].as_array().unwrap().clone();
    assert_eq!(unmapped.len(), 2);
    assert_eq!(unmapped[0]["old_unit"], "Einzel Vorlauf Expert");
    // They count for nothing until resolved.
    assert_eq!(anna["registered_deployments"], 0);

    let resolve = |id: &Value, unit: Value| {
        let app = &app;
        let admin = &admin;
        let body = json!({ "id": id, "unit": unit });
        async move {
            app.post(
                "/api/command/resolve_unmapped_registration",
                body,
                Some(admin),
            )
            .await
        }
    };
    let forbidden = app
        .post(
            "/api/command/resolve_unmapped_registration",
            json!({ "id": unmapped[0]["id"], "unit": null }),
            Some(&owner),
        )
        .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    let moved = resolve(
        &unmapped[0]["id"],
        json!({ "kind": "heat", "id": heat["id"] }),
    )
    .await;
    assert_eq!(moved.status, StatusCode::OK, "{:?}", moved.body);
    // The other one is dropped.
    let dropped = resolve(&unmapped[1]["id"], Value::Null).await;
    assert_eq!(dropped.status, StatusCode::OK, "{:?}", dropped.body);
    let anna = judges(&app, &owner, club_id).await[0].clone();
    assert_eq!(anna["unmapped_registrations"], json!([]));
    assert_eq!(anna["registered_deployments"], 2);
    let gone = resolve(&unmapped[1]["id"], Value::Null).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}

async fn club_of(app: &TestApp, cookie: &str) -> Value {
    let response = app.get("/api/query/get_club", Some(cookie)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body
}

async fn skip_heat(app: &TestApp, admin: &str, name: &str) {
    let skipped = app
        .post(
            "/api/command/set_heat_skip",
            json!({ "id": app.heat_id(name).await, "skip": true }),
            Some(admin),
        )
        .await;
    assert_eq!(skipped.status, StatusCode::OK, "{:?}", skipped.body);
}

#[tokio::test]
async fn a_flagged_registration_may_stay_but_gets_no_new_task() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let heat = unit(&app, &admin, "JE EW").await;
    let added = add_judge(
        &app,
        &owner,
        judge_body(club_id, "Anna", &[(&heat, "technical")]),
    )
    .await;
    assert_eq!(added.status, StatusCode::OK, "{:?}", added.body);
    skip_heat(&app, &admin, "JE EW").await;

    // Another task on the skipped Vorlauf is a new registration.
    for task in ["performance", "dismounts"] {
        let mut edit = judge_body(club_id, "Anna", &[(&heat, "technical"), (&heat, task)]);
        edit.as_object_mut().unwrap().remove("club_id");
        edit["judge_id"] = added.body["judge_id"].clone();
        let response = app
            .post("/api/command/edit_club_judge", edit, Some(&owner))
            .await;
        rejected(&response, "kann niemand mehr gemeldet werden");
    }
    let anna = judges(&app, &owner, club_id).await[0].clone();
    assert_eq!(anna["registrations"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_jury_lead_cannot_move_registrations_to_a_unit_that_takes_none() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let from = unit(&app, &admin, "E EW").await;
    let skipped = unit(&app, &admin, "JE EW").await;
    let added = add_judge(
        &app,
        &owner,
        judge_body(club_id, "Anna", &[(&from, "technical")]),
    )
    .await;
    let judge_id = added.body["judge_id"].clone();
    skip_heat(&app, &admin, "JE EW").await;
    // Also a unit switched off.
    let final_unit = unit(&app, &admin, "Finale EW").await;
    app.post(
        "/api/command/edit_jury_unit",
        json!({
            "kind": "final", "id": final_unit["id"], "registrable": false, "deployments": 1,
            "needed_technical": 5, "needed_performance": 5, "needed_dismounts": 3,
        }),
        Some(&admin),
    )
    .await;

    for target in [&skipped, &final_unit] {
        let moved = app
            .post(
                "/api/command/move_judge_registrations",
                json!({
                    "judge_id": judge_id,
                    "from_kind": "heat", "from_id": from["id"],
                    "to_kind": target["kind"], "to_id": target["id"],
                }),
                Some(&admin),
            )
            .await;
        rejected(&moved, "kann niemand mehr gemeldet werden");
    }
    let anna = judges(&app, &owner, club_id).await[0].clone();
    assert_eq!(anna["registrations"][0]["unit_id"], from["id"]);

    // The same for an unmapped registration.
    sqlx::query(
        "INSERT INTO judge_registration_unmapped (judge_id, old_unit, task) VALUES ($1, 'Alt', 'technical')",
    )
    .bind(Uuid::parse_str(judge_id.as_str().unwrap()).unwrap())
    .execute(&app.db)
    .await
    .unwrap();
    let anna = judges(&app, &owner, club_id).await[0].clone();
    let resolved = app
        .post(
            "/api/command/resolve_unmapped_registration",
            json!({
                "id": anna["unmapped_registrations"][0]["id"],
                "unit": { "kind": "heat", "id": skipped["id"] },
            }),
            Some(&admin),
        )
        .await;
    rejected(&resolved, "kann niemand mehr gemeldet werden");
    let anna = judges(&app, &owner, club_id).await[0].clone();
    assert_eq!(anna["unmapped_registrations"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn skipping_a_heat_leaves_the_clubs_obligation_and_fee_unchanged() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    for i in 0..3 {
        app.add_rider(
            &owner,
            common::rider_body(club_id, &format!("Fahrerin{i}"), true, false),
        )
        .await;
    }
    let heat = unit(&app, &admin, "JE EW").await;
    let other = unit(&app, &admin, "E EW").await;
    let added = add_judge(
        &app,
        &owner,
        judge_body(
            club_id,
            "Anna",
            &[(&heat, "technical"), (&other, "performance")],
        ),
    )
    .await;
    assert_eq!(added.status, StatusCode::OK, "{:?}", added.body);
    let before = club_of(&app, &owner).await;
    assert_eq!(before["jury"]["registered"], 4);
    assert_eq!(before["jury"]["missing"], 4);

    skip_heat(&app, &admin, "JE EW").await;
    let after = club_of(&app, &owner).await;
    assert_eq!(after["jury"], before["jury"]);
    assert_eq!(after["amount_due"], before["amount_due"]);
}

#[tokio::test]
async fn jury_units_show_the_readable_name_and_carry_the_code() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let response = app
        .post(
            "/api/command/edit_heat",
            json!({
                "id": app.heat_id("JE EW").await,
                "new_name": "Einzel weiblich – Junior Expert (U11–U15)",
                "description": null,
                "categories": ["EWU11", "EWU13", "EWU15"],
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let heat = unit(&app, &admin, "Einzel weiblich – Junior Expert (U11–U15)").await;
    assert_eq!(heat["kind"], "heat");
    assert_eq!(heat["code"], "JE EW");
    // A final without a rename: the name is its code.
    let final_unit = unit(&app, &admin, "Finale EW").await;
    assert_eq!(final_unit["code"], "Finale EW");
}
