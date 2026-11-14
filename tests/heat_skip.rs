//! Integration tests for skipping a heat's Vorlauf (#344): a heat with too
//! few acts starts directly in its final (Konzept §1.5).

mod common;

use axum::http::StatusCode;
use common::{TestApp, TestResponse};
use einrad_server::ids::{ActId, ClubId, FinalId, HeatId};
use serde_json::{Value, json};
use sqlx::SqlitePool;

/// The migration that made heats their own entity (#338).
const HEATS: i64 = 20261112120000;

/// The final in `tests/fixtures/competition.toml`, fed by "JE EW" (EWU11,
/// EWU13, EWU15) and "E EW" (EWU17).
const FINAL: &str = "Finale EW";

/// Format sent by the admin frontend.
const START: &str = "2026-04-19T09:00:00Z";

fn rejected(response: &TestResponse, message: &str) {
    assert_ne!(response.status, StatusCode::OK, "{:?}", response.body);
    assert!(
        response.body.to_string().contains(message),
        "expected „{message}“ in {:?}",
        response.body
    );
}

fn ok(response: &TestResponse) {
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

async fn list_heats(app: &TestApp) -> Vec<Value> {
    let response = app.get("/api/query/list_heats", None).await;
    ok(&response);
    response.body.as_array().unwrap().clone()
}

async fn heat(app: &TestApp, name: &str) -> Value {
    list_heats(app)
        .await
        .into_iter()
        .find(|heat| heat["name"] == name)
        .unwrap()
}

// The migration.

async fn insert_category(db: &SqlitePool, name: &str, under_age: Option<f64>, heat: &str) {
    sqlx::query(
        "INSERT INTO categories (name, act_type, is_single_male, under_age, heat) \
         VALUES (?, 'single', TRUE, ?, ?)",
    )
    .bind(name)
    .bind(under_age)
    .bind(heat)
    .execute(db)
    .await
    .unwrap();
}

async fn thresholds(db: &SqlitePool) -> Vec<(String, i64, bool)> {
    sqlx::query_as("SELECT name, threshold, skip FROM heats ORDER BY name")
        .fetch_all(db)
        .await
        .unwrap()
}

#[tokio::test]
async fn the_migration_gives_junior_expert_heats_6_and_others_5() {
    let db = common::in_memory().await;
    let migrator = sqlx::migrate!("./migrations");
    migrator.run_to(HEATS, &db).await.unwrap();
    for heat in ["JE EM", "E EM", "Gemischt", "Leer"] {
        sqlx::query("INSERT INTO heats (name) VALUES (?)")
            .bind(heat)
            .execute(&db)
            .await
            .unwrap();
    }
    insert_category(&db, "EMU13", Some(13.0), "JE EM").await;
    insert_category(&db, "EMU15", Some(15.0), "JE EM").await;
    insert_category(&db, "EMU17", Some(17.0), "E EM").await;
    insert_category(&db, "EMU11", Some(11.0), "Gemischt").await;
    insert_category(&db, "EM70+", None, "Gemischt").await;
    // A block with a break the organizer planned: it stays.
    sqlx::query(
        "INSERT INTO timeplan (id, kind, category, position) VALUES (1, 'acts', 'EMU13', 1)",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO timeplan_slots (timeplan_id, position, label, duration_seconds) \
         VALUES (1, 0, 'Einfahren', 300)",
    )
    .execute(&db)
    .await
    .unwrap();

    migrator.run(&db).await.unwrap();

    assert_eq!(
        thresholds(&db).await,
        [
            ("E EM".to_string(), 5, false),
            ("Gemischt".to_string(), 5, false),
            ("JE EM".to_string(), 6, false),
            ("Leer".to_string(), 5, false),
        ]
    );
    let breaks: Vec<String> = sqlx::query_scalar("SELECT label FROM timeplan_slots")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(breaks, ["Einfahren"]);

    migrator.undo(&db, HEATS).await.unwrap();
    let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('heats')")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(columns, ["name", "description"]);
    let breaks: Vec<String> = sqlx::query_scalar("SELECT label FROM timeplan_slots")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(breaks, ["Einfahren"]);
}

// The heats.

#[tokio::test]
async fn heats_show_their_acts_threshold_and_the_suggestion() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    // Singles female of the default birthdate start in EWU13 ("JE EW").
    for name in ["Anna", "Berta", "Carla"] {
        app.add_single_act(&owner, club_id, name).await;
    }

    let heats = list_heats(&app).await;
    let fields = |heat: &Value| {
        (
            heat["name"].as_str().unwrap().to_string(),
            heat["threshold"].clone(),
            heat["skip"].clone(),
            heat["act_count"].clone(),
            heat["skip_suggested"].clone(),
        )
    };
    assert_eq!(
        heats.iter().map(fields).collect::<Vec<_>>(),
        [
            (
                "JE EW".to_string(),
                json!(6),
                json!(false),
                json!(3),
                json!(true)
            ),
            (
                "E EW".to_string(),
                json!(5),
                json!(false),
                json!(0),
                json!(true)
            ),
        ]
    );
}

#[tokio::test]
async fn the_import_preview_names_a_changed_threshold() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let file = include_str!("fixtures/competition.toml").replacen(
        "categories = [\"EWU17\"]",
        "categories = [\"EWU17\"]\nthreshold = 3",
        1,
    );
    let response = app
        .post(
            "/api/command/preview_competition_import",
            json!({ "file": file }),
            Some(&admin),
        )
        .await;
    ok(&response);
    let changes = response.body["changes"].as_array().unwrap();
    assert!(changes.contains(&json!("heats")), "{changes:?}");
}

#[tokio::test]
async fn new_heats_get_the_default_threshold_unless_given_one() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    for body in [
        json!({ "name": "JE EM", "description": null, "categories": ["EMU11", "EMU13", "EMU15"] }),
        json!({ "name": "E EM", "description": null, "categories": ["EMU17"] }),
        json!({ "name": "Leer", "description": null, "categories": [] }),
        json!({ "name": "Paare", "description": null, "categories": ["PU11"], "threshold": 2 }),
    ] {
        ok(&app.post("/api/command/add_heat", body, Some(&admin)).await);
    }
    let thresholds: Vec<(Value, Value)> = list_heats(&app)
        .await
        .iter()
        .map(|heat| (heat["name"].clone(), heat["threshold"].clone()))
        .collect();
    for (name, threshold) in [("JE EM", 6), ("E EM", 5), ("Leer", 5), ("Paare", 2)] {
        assert!(
            thresholds.contains(&(json!(name), json!(threshold))),
            "{name}: {thresholds:?}"
        );
    }

    rejected(
        &app.post(
            "/api/command/add_heat",
            json!({ "name": "X", "description": null, "categories": [], "threshold": -1 }),
            Some(&admin),
        )
        .await,
        "negativ",
    );
}

#[tokio::test]
async fn the_organizer_changes_the_threshold() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let id = app.heat_id("E EW").await;
    let edit = |threshold: i64| {
        json!({
            "id": id,
            "new_name": "E EW",
            "description": null,
            "categories": ["EWU17"],
            "threshold": threshold,
        })
    };
    ok(&app
        .post("/api/command/edit_heat", edit(3), Some(&admin))
        .await);
    assert_eq!(heat(&app, "E EW").await["threshold"], 3);
    assert_eq!(heat(&app, "E EW").await["skip_suggested"], true);
    rejected(
        &app.post("/api/command/edit_heat", edit(-1), Some(&admin))
            .await,
        "negativ",
    );
    // Without a threshold, it stays.
    let response = app
        .post(
            "/api/command/edit_heat",
            json!({ "id": id, "new_name": "E EW", "description": null, "categories": ["EWU17"] }),
            Some(&admin),
        )
        .await;
    ok(&response);
    assert_eq!(heat(&app, "E EW").await["threshold"], 3);
}

/// The id of the heat `name`; a made-up one for a heat that doesn't exist.
async fn heat_id_or_unknown(app: &TestApp, name: &str) -> HeatId {
    sqlx::query_scalar("SELECT id FROM heats WHERE name = ?")
        .bind(name)
        .fetch_optional(&app.db)
        .await
        .unwrap()
        .unwrap_or_else(HeatId::new)
}

async fn set_skip(app: &TestApp, admin: &str, name: &str, skip: bool) -> TestResponse {
    app.post(
        "/api/command/set_heat_skip",
        json!({ "id": heat_id_or_unknown(app, name).await, "skip": skip }),
        Some(admin),
    )
    .await
}

#[tokio::test]
async fn the_organizer_skips_and_restores_a_vorlauf() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, _) = app.create_club_owner("Owner", "RSV").await;

    ok(&set_skip(&app, &admin, "JE EW", true).await);
    assert_eq!(heat(&app, "JE EW").await["skip"], true);
    // The suggestion is just that: the tool never switches by itself.
    assert_eq!(heat(&app, "E EW").await["skip"], false);
    ok(&set_skip(&app, &admin, "JE EW", false).await);
    assert_eq!(heat(&app, "JE EW").await["skip"], false);

    assert_eq!(
        set_skip(&app, &admin, "Gibt es nicht", true).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        set_skip(&app, &owner, "JE EW", true).await.status,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn a_started_vorlauf_or_final_keeps_its_skip() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let anna = app.add_single_act(&owner, club_id, "Anna").await;

    // An act of the heat has started.
    sqlx::query("UPDATE acts SET started_at = datetime('now') WHERE id = ?")
        .bind(anna)
        .execute(&app.db)
        .await
        .unwrap();
    rejected(&set_skip(&app, &admin, "JE EW", true).await, "begonnen");
    sqlx::query("UPDATE acts SET started_at = NULL WHERE id = ?")
        .bind(anna)
        .execute(&app.db)
        .await
        .unwrap();

    // A slot of its final has started: also for the other heat of the final.
    sqlx::query(
        "UPDATE final_slots SET started_at = datetime('now') \
         WHERE id = (SELECT final_slots.id FROM final_slots \
            JOIN finals ON finals.id = final_slots.final_id \
            WHERE finals.name = ? ORDER BY position LIMIT 1)",
    )
    .bind(FINAL)
    .execute(&app.db)
    .await
    .unwrap();
    rejected(&set_skip(&app, &admin, "E EW", true).await, "begonnen");
}

/// The body to add or (for a final that exists) edit the final `name` with
/// one slot for each of the heats named.
async fn final_body(app: &TestApp, name: &str, heats: &[&str]) -> Value {
    let id: Option<FinalId> = sqlx::query_scalar("SELECT id FROM finals WHERE name = ?")
        .bind(name)
        .fetch_optional(&app.db)
        .await
        .unwrap();
    let mut groups = vec![];
    for heat in heats {
        groups.push(json!({ "heat_id": app.heat_id(heat).await, "count": 1 }));
    }
    json!({
        "id": id,
        "name": name,
        "new_name": name,
        "description": null,
        "act_type": "single",
        "is_single_male": false,
        "einfahrzeit_seconds": 300,
        "act_duration_seconds": 180,
        "judge_duration_seconds": 120,
        "slot_groups": groups,
    })
}

#[tokio::test]
async fn a_heat_fills_at_most_one_final() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    ok(&app
        .post(
            "/api/command/add_heat",
            json!({ "name": "E EW 2", "description": null, "categories": ["EWU19"] }),
            Some(&admin),
        )
        .await);

    rejected(
        &app.post(
            "/api/command/add_final",
            final_body(&app, "Finale 2", &["E EW 2", "E EW"]).await,
            Some(&admin),
        )
        .await,
        "„Finale EW“",
    );
    ok(&app
        .post(
            "/api/command/add_final",
            final_body(&app, "Finale 2", &["E EW 2"]).await,
            Some(&admin),
        )
        .await);
    rejected(
        &app.post(
            "/api/command/edit_final",
            final_body(&app, "Finale 2", &["E EW 2", "JE EW"]).await,
            Some(&admin),
        )
        .await,
        "„Finale EW“",
    );
    // Its own heats, also twice, are fine.
    ok(&app
        .post(
            "/api/command/edit_final",
            final_body(&app, FINAL, &["JE EW", "E EW", "JE EW"]).await,
            Some(&admin),
        )
        .await);
}

// The timeplan.

/// Adds a single female act in EWU17 ("E EW") and returns its id.
async fn add_ewu17_act(app: &TestApp, owner: &str, club_id: ClubId, firstname: &str) -> ActId {
    let mut body = common::rider_body(club_id, firstname, true, false);
    body["birthdate"] = json!("2011-05-01T00:00:00.000Z");
    let rider = app.add_rider(owner, body).await;
    app.act_of(rider).await
}

async fn replace_timeplan(app: &TestApp, admin: &str, entries: &[Value]) {
    let response = app.get("/api/query/list_timeplan", None).await;
    for entry in response.body.as_array().unwrap() {
        ok(&app
            .post(
                "/api/command/delete_timeplan_entry",
                json!({ "id": entry["id"] }),
                Some(admin),
            )
            .await);
    }
    for entry in entries {
        ok(&app
            .post(
                "/api/command/add_timeplan_entry",
                entry.clone(),
                Some(admin),
            )
            .await);
    }
}

async fn block(
    app: &TestApp,
    category: Option<&str>,
    final_name: Option<&str>,
    start: Option<&str>,
) -> Value {
    let final_id = match final_name {
        Some(name) => Some(app.final_id(name).await),
        None => None,
    };
    json!({
        "earliest_start_time": start,
        "duration_seconds": null,
        "label": null,
        "category": category,
        "final_id": final_id,
    })
}

async fn predicted(app: &TestApp, cookie: Option<&str>, public: bool) -> Vec<Value> {
    let uri = if public {
        "/api/query/predict_timeplan?public=true"
    } else {
        "/api/query/predict_timeplan"
    };
    let response = app.get(uri, cookie).await;
    ok(&response);
    response.body["items"].as_array().unwrap().clone()
}

/// What runs in a predicted block: "act:<id>", a finalist's label or a
/// break's.
fn slots(item: &Value) -> Vec<String> {
    item["timeplan_entry"]["Acts"]["slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|slot| {
            if let Some(act) = slot.get("Act") {
                format!("act:{}", act["id"].as_str().unwrap())
            } else if let Some(finalist) = slot.get("Finalist") {
                finalist["label"].as_str().unwrap().to_string()
            } else {
                slot["Break"]["label"].as_str().unwrap().to_string()
            }
        })
        .collect()
}

fn skipped(item: &Value) -> bool {
    item["timeplan_entry"]["Acts"]["preliminary_skipped"]
        .as_bool()
        .unwrap()
}

fn act(id: ActId) -> String {
    format!("act:{id}")
}

#[tokio::test]
async fn a_skipped_heat_starts_directly_in_its_final() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    common::set_startlist_published(&app.db, true).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let anna = app.add_single_act(&owner, club_id, "Anna").await;
    let berta = app.add_single_act(&owner, club_id, "Berta").await;
    let dora = add_ewu17_act(&app, &owner, club_id, "Dora").await;
    replace_timeplan(
        &app,
        &admin,
        &[
            block(&app, Some("EWU13"), None, Some(START)).await,
            block(&app, Some("EWU17"), None, None).await,
            block(&app, None, Some(FINAL), None).await,
        ],
    )
    .await;
    let before = predicted(&app, Some(&admin), false).await;
    assert_eq!(slots(&before[0]), [act(anna), act(berta)]);
    assert!(!skipped(&before[0]));

    ok(&set_skip(&app, &admin, "JE EW", true).await);

    // Its blocks stay, marked, and take no time; the final holds its acts
    // instead of its finalist slots, without a published placement.
    for items in [
        predicted(&app, Some(&admin), false).await,
        predicted(&app, None, true).await,
    ] {
        assert!(skipped(&items[0]));
        assert!(slots(&items[0]).is_empty());
        assert_eq!(items[0]["planned_duration"], 0);
        assert_eq!(items[1]["planned_start"], items[0]["planned_start"]);
        assert!(!skipped(&items[1]));
        assert_eq!(slots(&items[1]), [act(dora)]);
        assert_eq!(
            slots(&items[2]),
            ["1. Finalist (E EW)".to_string(), act(anna), act(berta)]
        );
    }
    // A direct act runs with its category's times, like any act.
    let items = predicted(&app, Some(&admin), false).await;
    let direct = &items[2]["timeplan_entry"]["Acts"]["slots"][1]["Act"];
    assert_eq!(direct["category"], "EWU13");
    let vorlauf = &before[0]["timeplan_entry"]["Acts"]["slots"][0]["Act"];
    assert_eq!(
        direct["judge_duration_seconds"],
        vorlauf["judge_duration_seconds"]
    );

    // Running the Vorlauf again restores it in place.
    ok(&set_skip(&app, &admin, "JE EW", false).await);
    let after = predicted(&app, Some(&admin), false).await;
    assert_eq!(slots(&after[0]), [act(anna), act(berta)]);
    assert!(!skipped(&after[0]));
    assert_eq!(
        slots(&after[2]),
        [
            "1. Finalist (JE EW)",
            "2. Finalist (JE EW)",
            "1. Finalist (E EW)"
        ]
    );
}

#[tokio::test]
async fn a_skipped_heat_without_a_planned_final_keeps_its_vorlauf() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let anna = app.add_single_act(&owner, club_id, "Anna").await;
    replace_timeplan(
        &app,
        &admin,
        &[block(&app, Some("EWU13"), None, Some(START)).await],
    )
    .await;

    ok(&set_skip(&app, &admin, "JE EW", true).await);

    let items = predicted(&app, Some(&admin), false).await;
    assert!(!skipped(&items[0]));
    assert_eq!(slots(&items[0]), [act(anna)]);
}

async fn set_order(app: &TestApp, admin: &str, timeplan_id: &Value, slots: Value) -> TestResponse {
    app.post(
        "/api/command/set_block_order",
        json!({ "timeplan_id": timeplan_id, "slots": slots }),
        Some(admin),
    )
    .await
}

async fn finalist(app: &TestApp, heat: &str, number: i64) -> Value {
    json!({ "Finalist": { "heat_id": app.heat_id(heat).await, "number": number } })
}

fn act_input(id: ActId) -> Value {
    json!({ "Act": { "act_id": id } })
}

#[tokio::test]
async fn direct_acts_and_finalist_slots_share_the_finals_start_order() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let anna = app.add_single_act(&owner, club_id, "Anna").await;
    let berta = app.add_single_act(&owner, club_id, "Berta").await;
    let dora = add_ewu17_act(&app, &owner, club_id, "Dora").await;
    replace_timeplan(
        &app,
        &admin,
        &[
            block(&app, None, Some(FINAL), Some(START)).await,
            block(&app, Some("EWU17"), None, None).await,
        ],
    )
    .await;
    ok(&set_skip(&app, &admin, "JE EW", true).await);
    let items = predicted(&app, Some(&admin), false).await;
    let (final_block, ewu17) = (items[0]["id"].clone(), items[1]["id"].clone());
    assert_eq!(
        slots(&items[0]),
        ["1. Finalist (E EW)".to_string(), act(anna), act(berta)]
    );

    // Like a category block's order: acts, finalist slots and breaks.
    let response = set_order(
        &app,
        &admin,
        &final_block,
        json!([
            act_input(berta),
            finalist(&app, "E EW", 1).await,
            { "Break": { "id": null, "label": "Umbau", "duration_seconds": 120 } },
            act_input(anna),
        ]),
    )
    .await;
    ok(&response);
    let items = predicted(&app, Some(&admin), false).await;
    assert_eq!(
        slots(&items[0]),
        [
            act(berta),
            "1. Finalist (E EW)".to_string(),
            "Umbau".to_string(),
            act(anna)
        ]
    );
    assert_eq!(items[0]["timeplan_entry"]["Acts"]["manually_ordered"], true);

    // The order survives new slot groups; new slots and acts left out go to
    // the end.
    ok(&set_order(
        &app,
        &admin,
        &final_block,
        json!([act_input(berta), finalist(&app, "E EW", 1).await]),
    )
    .await);
    let mut body = final_body(&app, FINAL, &["JE EW", "JE EW", "E EW", "E EW"]).await;
    body["slot_groups"] = json!([
        { "heat_id": app.heat_id("JE EW").await, "count": 2 },
        { "heat_id": app.heat_id("E EW").await, "count": 2 },
    ]);
    ok(&app
        .post("/api/command/edit_final", body, Some(&admin))
        .await);
    assert_eq!(
        slots(&predicted(&app, Some(&admin), false).await[0]),
        [
            act(berta),
            "1. Finalist (E EW)".to_string(),
            "2. Finalist (E EW)".to_string(),
            act(anna)
        ]
    );

    // Only the final's own starts: no acts of heats that run their
    // Vorlauf, no slots of skipped heats.
    for (slots, message) in [
        (json!([act_input(dora)]), "Finale"),
        (json!([finalist(&app, "JE EW", 1).await]), "JE EW"),
        (json!([finalist(&app, "E EW", 9).await]), "E EW"),
    ] {
        rejected(&set_order(&app, &admin, &final_block, slots).await, message);
    }
    // Direct acts stay in the final, finalist slots too.
    for slots in [
        json!([act_input(anna)]),
        json!([finalist(&app, "E EW", 1).await]),
    ] {
        rejected(&set_order(&app, &admin, &ewu17, slots).await, "Finale");
    }

    // Resetting gives the final's own order back.
    ok(&app
        .post(
            "/api/command/reset_block_order",
            json!({ "timeplan_id": final_block }),
            Some(&admin),
        )
        .await);
    assert_eq!(
        slots(&predicted(&app, Some(&admin), false).await[0]),
        [
            "1. Finalist (E EW)".to_string(),
            "2. Finalist (E EW)".to_string(),
            act(anna),
            act(berta)
        ]
    );
}

#[tokio::test]
async fn a_direct_act_runs_like_an_act_in_the_final() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let anna = app.add_single_act(&owner, club_id, "Anna").await;
    replace_timeplan(
        &app,
        &admin,
        &[block(&app, None, Some(FINAL), Some(START)).await],
    )
    .await;
    ok(&set_skip(&app, &admin, "JE EW", true).await);
    let final_block = predicted(&app, Some(&admin), false).await[0]["id"].clone();
    ok(&set_order(&app, &admin, &final_block, json!([act_input(anna)])).await);

    // The warm-up, then Anna.
    for _ in 0..2 {
        ok(&app
            .post("/api/command/timeplan_forward", json!(null), Some(&admin))
            .await);
    }
    let items = predicted(&app, Some(&admin), false).await;
    let first = &items[0]["timeplan_entry"]["Acts"]["slots"][0]["Act"];
    assert_eq!(first["id"], json!(anna));
    assert_eq!(first["status"], "Started");
    // Started: the skip stays.
    rejected(&set_skip(&app, &admin, "JE EW", false).await, "begonnen");
}

/// Edits the slot groups of the final and tries to delete it.
async fn change_and_delete_final(app: &TestApp, admin: &str) -> (TestResponse, TestResponse) {
    let mut body = final_body(app, FINAL, &["JE EW", "E EW"]).await;
    body["slot_groups"] = json!([
        { "heat_id": app.heat_id("JE EW").await, "count": 3 },
        { "heat_id": app.heat_id("E EW").await, "count": 1 },
    ]);
    let edited = app.post("/api/command/edit_final", body, Some(admin)).await;
    let deleted = app
        .post(
            "/api/command/delete_final",
            json!({ "id": app.final_id(FINAL).await }),
            Some(admin),
        )
        .await;
    (edited, deleted)
}

#[tokio::test]
async fn a_final_with_only_direct_acts_is_started_with_its_block() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    app.add_single_act(&owner, club_id, "Anna").await;
    replace_timeplan(
        &app,
        &admin,
        &[block(&app, None, Some(FINAL), Some(START)).await],
    )
    .await;
    ok(&set_skip(&app, &admin, "JE EW", true).await);
    sqlx::query("UPDATE timeplan SET started_at = datetime('now')")
        .execute(&app.db)
        .await
        .unwrap();

    let (edited, deleted) = change_and_delete_final(&app, &admin).await;
    rejected(&edited, "schon begonnen");
    assert_eq!(deleted.status, StatusCode::CONFLICT, "{:?}", deleted.body);
    assert!(deleted.body.to_string().contains("schon begonnen"));
}

#[tokio::test]
async fn a_final_is_started_when_a_direct_act_started() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let anna = app.add_single_act(&owner, club_id, "Anna").await;
    ok(&set_skip(&app, &admin, "JE EW", true).await);
    sqlx::query("UPDATE acts SET started_at = datetime('now') WHERE id = ?")
        .bind(anna)
        .execute(&app.db)
        .await
        .unwrap();

    let (edited, deleted) = change_and_delete_final(&app, &admin).await;
    rejected(&edited, "schon begonnen");
    assert_eq!(deleted.status, StatusCode::CONFLICT, "{:?}", deleted.body);
}
