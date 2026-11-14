//! Integration tests for the jury registration (#225): the feature flag, the
//! jury rules, jury units (heats and finals), judges, the obligation and the
//! Juryplan.

mod common;

use axum::http::StatusCode;
use common::{TestApp, TestResponse, closed_registration};
use serde_json::{
    Value::{self, Null},
    json,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

async fn system_status(app: &TestApp, cookie: Option<&str>) -> Value {
    let response = app.get("/api/query/get_system_status", cookie).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body
}

#[tokio::test]
async fn system_status_shows_the_flag_rules_and_judge_window() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let status = system_status(&app, None).await;
    // Tests are debug builds, where the flag is on by default.
    assert_eq!(status["jury_registration"], true);
    assert_eq!(status["can_register_judge"], true);
    assert_eq!(
        status["competition"]["jury"],
        json!({
            "min_age": 15,
            "deployments_per_two_acts": 4,
            "missing_deployment_fee": 7.0,
            "registration_end": null,
        })
    );
}

#[tokio::test]
async fn judges_follow_the_registration_window_unless_the_jury_has_its_own() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_status(db, closed_registration()).await;
    assert_eq!(system_status(&app, None).await["can_register_judge"], false);
    let admin = app.create_admin().await;
    assert_eq!(
        system_status(&app, Some(&admin)).await["can_register_judge"],
        true
    );

    // A later jury deadline keeps the judge registration open.
    let later = OffsetDateTime::now_utc() + Duration::days(3);
    sqlx::query("UPDATE competition SET jury_registration_end = ?")
        .bind(later)
        .execute(&app.db)
        .await
        .unwrap();
    let status = system_status(&app, None).await;
    assert_eq!(status["can_register_judge"], true);
    assert_eq!(status["can_register_rider"], false);
}

#[tokio::test]
async fn the_flag_can_be_turned_off() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_jury_registration(db).await;
    let status = system_status(&app, None).await;
    assert_eq!(status["jury_registration"], false);
}

#[tokio::test]
async fn the_competitions_feature_applies_on_the_next_request() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_jury_registration(db).await;
    let admin = app.create_admin().await;
    let categories = "/api/query/list_jury_units";
    assert_eq!(
        app.get(categories, Some(&admin)).await.status,
        StatusCode::NOT_FOUND
    );

    common::set_jury_registration(&app.db, Some(true)).await;
    assert_eq!(system_status(&app, None).await["jury_registration"], true);
    assert_eq!(
        system_status(&app, None).await["competition"]["features"],
        json!({ "jury_registration": true, "moderation_input": Null })
    );
    assert_eq!(
        app.get(categories, Some(&admin)).await.status,
        StatusCode::OK
    );

    common::set_jury_registration(&app.db, Some(false)).await;
    assert_eq!(system_status(&app, None).await["jury_registration"], false);
    assert_eq!(
        app.get(categories, Some(&admin)).await.status,
        StatusCode::NOT_FOUND
    );

    // Unset follows the build default: on in debug builds, off in release.
    common::set_jury_registration(&app.db, None).await;
    assert_eq!(
        system_status(&app, None).await["jury_registration"],
        cfg!(debug_assertions)
    );
}

#[tokio::test]
async fn the_competition_can_turn_off_what_the_build_default_turns_on() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    common::set_jury_registration(&app.db, Some(false)).await;
    assert_eq!(system_status(&app, None).await["jury_registration"], false);
    assert_eq!(
        app.get("/api/query/list_jury_units", Some(&admin))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

// ---------- Jury units (#227, #347) ----------

async fn jury_units(app: &TestApp, cookie: &str) -> Vec<Value> {
    let response = app.get("/api/query/list_jury_units", Some(cookie)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_array().unwrap().clone()
}

fn unit_body(unit: &Value, deployments: i64) -> Value {
    json!({
        "kind": unit["kind"],
        "id": unit["id"],
        "registrable": true,
        "deployments": deployments,
        "needed_technical": 5,
        "needed_performance": 5,
        "needed_dismounts": 3,
    })
}

#[tokio::test]
async fn every_heat_and_final_is_a_jury_unit_in_timeplan_order() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, _) = app.create_club_owner("Owner", "RSV").await;
    let units = jury_units(&app, &owner).await;
    let summary: Vec<Value> = units
        .iter()
        .map(|u| {
            json!([
                u["kind"],
                u["name"],
                u["deployments"],
                u["day"],
                u["registrable"],
                u["needed_technical"],
                u["needed_performance"],
                u["needed_dismounts"],
            ])
        })
        .collect();
    // The heats run in the timeplan on the first day, the final isn't
    // planned yet: no day, and it comes last.
    assert_eq!(
        summary,
        vec![
            json!(["heat", "JE EW", 2, "2027-06-05", true, 5, 5, 3]),
            json!(["heat", "E EW", 2, "2027-06-05", true, 5, 5, 3]),
            json!(["final", "Finale EW", 1, null, true, 3, 3, 2]),
        ]
    );

    let anonymous = app.get("/api/query/list_jury_units", None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn units_say_their_discipline_and_the_final_a_heat_feeds() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, _) = app.create_club_owner("Owner", "RSV").await;
    // A heat that feeds no final, one that has no categories yet.
    sqlx::query("INSERT INTO heats (id, name, code) VALUES (x'aa000000000000000000000000000001', 'Paare', 'PA'), (x'aa000000000000000000000000000002', 'Leer', 'LE')")
        .execute(&app.db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE categories SET heat_id = x'aa000000000000000000000000000001' WHERE name = 'PU11'",
    )
    .execute(&app.db)
    .await
    .unwrap();
    let units = jury_units(&app, &owner).await;
    let final_id = units.iter().find(|u| u["kind"] == "final").unwrap()["id"].clone();
    let summary: Vec<Value> = units
        .iter()
        .map(|u| json!([u["name"], u["act_type"], u["final_id"]]))
        .collect();
    assert_eq!(
        summary,
        vec![
            json!(["JE EW", "single", final_id]),
            json!(["E EW", "single", final_id]),
            json!(["Paare", "pair", null]),
            json!(["Leer", null, null]),
            json!(["Finale EW", "single", null]),
        ]
    );
}

#[tokio::test]
async fn the_day_of_a_unit_follows_the_timeplan() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    // E EW (EWU17) starts on the second day, JE EW (EWU11) on the first.
    sqlx::query(
        "UPDATE timeplan SET earliest_start_time = '2027-06-06T09:00:00+02:00' WHERE category_id = (SELECT id FROM categories WHERE name = 'EWU17')",
    )
    .execute(&app.db)
    .await
    .unwrap();
    // The final's block on the second day, too.
    sqlx::query(
        "INSERT INTO timeplan (kind, final_id, position, earliest_start_time) \
         VALUES ('acts', (SELECT id FROM finals WHERE name = 'Finale EW'), \
         (SELECT MAX(position) + 1 FROM timeplan), '2027-06-06T15:00:00+02:00')",
    )
    .execute(&app.db)
    .await
    .unwrap();
    let units = jury_units(&app, &admin).await;
    let days: Vec<Value> = units.iter().map(|u| json!([u["name"], u["day"]])).collect();
    assert_eq!(
        days,
        vec![
            json!(["JE EW", "2027-06-05"]),
            json!(["E EW", "2027-06-06"]),
            json!(["Finale EW", "2027-06-06"]),
        ]
    );
}

#[tokio::test]
async fn the_jury_lead_changes_the_settings_of_a_unit() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, _) = app.create_club_owner("Owner", "RSV").await;
    let units = jury_units(&app, &admin).await;
    let heat = &units[0];

    let forbidden = app
        .post(
            "/api/command/edit_jury_unit",
            unit_body(heat, 3),
            Some(&owner),
        )
        .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);

    let mut body = unit_body(heat, 3);
    body["needed_technical"] = json!(7);
    let edited = app
        .post("/api/command/edit_jury_unit", body, Some(&admin))
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{:?}", edited.body);
    let changed = &jury_units(&app, &admin).await[0];
    assert_eq!(changed["deployments"], 3);
    assert_eq!(changed["needed_technical"], 7);

    // Switched off, nobody can register for it.
    let mut off = unit_body(heat, 3);
    off["registrable"] = json!(false);
    app.post("/api/command/edit_jury_unit", off, Some(&admin))
        .await;
    let changed = &jury_units(&app, &admin).await[0];
    assert_eq!(changed["registrable"], false);
    assert_eq!(changed["registrable_setting"], false);

    // Negative numbers and unknown units are refused.
    let negative = app
        .post(
            "/api/command/edit_jury_unit",
            unit_body(heat, -1),
            Some(&admin),
        )
        .await;
    assert!(!negative.status.is_success());
    let mut unknown = unit_body(heat, 2);
    unknown["id"] = json!(Uuid::new_v4());
    let unknown = app
        .post("/api/command/edit_jury_unit", unknown, Some(&admin))
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn jury_endpoints_dont_exist_without_the_flag() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_jury_registration(db).await;
    let admin = app.create_admin().await;
    let list = app.get("/api/query/list_jury_units", Some(&admin)).await;
    assert_eq!(list.status, StatusCode::NOT_FOUND);
    let edit = app
        .post(
            "/api/command/edit_jury_unit",
            json!({
                "kind": "heat", "id": Uuid::new_v4(), "registrable": true, "deployments": 2,
                "needed_technical": 5, "needed_performance": 5, "needed_dismounts": 3,
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(edit.status, StatusCode::NOT_FOUND);
}

// ---------- Judges (#228) ----------

/// Body for `add_club_judge`; `registrations` are (jury unit, task).
fn judge_body(
    club_id: Uuid,
    firstname: &str,
    birthdate: &str,
    is_trained: bool,
    registrations: &[(&Value, &str)],
) -> Value {
    json!({
        "club_id": club_id,
        "firstname": firstname,
        "lastname": "Jury",
        "birthdate": birthdate,
        "is_trained": is_trained,
        "max_deployments": null,
        "note": null,
        "registrations": registrations
            .iter()
            .map(|(unit, task)| json!({ "unit_kind": unit["kind"], "unit_id": unit["id"], "task": task }))
            .collect::<Vec<_>>(),
    })
}

/// Born so that the judge is 30 on the first competition day.
const ADULT: &str = "1997-01-01T00:00:00.000Z";

/// The jury units: the heats "JE EW" and "E EW" (2 deployments each), then
/// the final "Finale EW" (1).
async fn category_ids(app: &TestApp, cookie: &str) -> Vec<Value> {
    jury_units(app, cookie).await
}

async fn add_judge(app: &TestApp, cookie: &str, body: Value) -> TestResponse {
    app.post("/api/command/add_club_judge", body, Some(cookie))
        .await
}

async fn club_judges(app: &TestApp, cookie: &str, club_id: Uuid) -> Vec<Value> {
    let response = app
        .get(
            &format!("/api/query/list_club_judges?club_id={club_id}"),
            Some(cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_array().unwrap().clone()
}

#[tokio::test]
async fn clubs_add_edit_and_delete_judges() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let ids = category_ids(&app, &owner).await;

    let mut body = judge_body(
        club_id,
        "Anna",
        ADULT,
        true,
        &[
            (&ids[0], "technical"),
            (&ids[0], "performance"),
            (&ids[1], "dismounts"),
        ],
    );
    body["max_deployments"] = json!(3);
    body["note"] = json!("Nur Samstag");
    let added = add_judge(&app, &owner, body).await;
    assert_eq!(added.status, StatusCode::OK, "{:?}", added.body);
    let judge_id = added.body["judge_id"].clone();

    let judges = club_judges(&app, &owner, club_id).await;
    assert_eq!(judges.len(), 1);
    let anna = &judges[0];
    assert_eq!(anna["id"], judge_id);
    assert_eq!(anna["firstname"], "Anna");
    assert_eq!(anna["is_trained"], true);
    assert_eq!(anna["max_deployments"], 3);
    assert_eq!(anna["note"], "Nur Samstag");
    assert_eq!(anna["registrations"].as_array().unwrap().len(), 3);

    // Editing replaces the registrations.
    let mut edit = judge_body(club_id, "Anna", ADULT, false, &[(&ids[2], "dismounts")]);
    edit.as_object_mut().unwrap().remove("club_id");
    edit["judge_id"] = judge_id.clone();
    let edited = app
        .post("/api/command/edit_club_judge", edit, Some(&owner))
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{:?}", edited.body);
    let anna = club_judges(&app, &owner, club_id).await[0].clone();
    assert_eq!(anna["is_trained"], false);
    assert_eq!(anna["note"], Value::Null);
    assert_eq!(
        anna["registrations"],
        json!([{ "unit_kind": ids[2]["kind"], "unit_id": ids[2]["id"], "task": "dismounts", "flagged": false }])
    );

    let deleted = app
        .post(
            "/api/command/delete_club_judge",
            json!({ "judge_id": judge_id }),
            Some(&owner),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::OK, "{:?}", deleted.body);
    assert!(club_judges(&app, &owner, club_id).await.is_empty());
}

#[tokio::test]
async fn only_the_club_and_admins_manage_its_judges() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let (other, _) = app.create_club_owner("Other", "RSV Andere").await;
    let ids = category_ids(&app, &owner).await;
    let body = judge_body(club_id, "Anna", ADULT, true, &[(&ids[0], "technical")]);

    assert_eq!(
        add_judge(&app, &other, body.clone()).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.post("/api/command/add_club_judge", body.clone(), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let judge_id = add_judge(&app, &owner, body).await.body["judge_id"].clone();

    let list = format!("/api/query/list_club_judges?club_id={club_id}");
    assert_eq!(
        app.get(&list, Some(&other)).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(app.get(&list, None).await.status, StatusCode::UNAUTHORIZED);

    let mut edit = judge_body(club_id, "Eve", ADULT, true, &[]);
    edit.as_object_mut().unwrap().remove("club_id");
    edit["judge_id"] = judge_id.clone();
    for (command, body) in [
        ("edit_club_judge", edit),
        ("delete_club_judge", json!({ "judge_id": judge_id })),
    ] {
        let response = app
            .post(&format!("/api/command/{command}"), body, Some(&other))
            .await;
        assert_eq!(response.status, StatusCode::FORBIDDEN, "{command}");
    }

    let admin = app.create_admin().await;
    assert_eq!(club_judges(&app, &admin, club_id).await.len(), 1);
    let all = app.get("/api/query/list_judges", Some(&admin)).await;
    assert_eq!(all.status, StatusCode::OK, "{:?}", all.body);
    assert_eq!(all.body[0]["club_name"], "RSV");
    assert_eq!(all.body[0]["firstname"], "Anna");
    assert_eq!(
        app.get("/api/query/list_judges", Some(&owner)).await.status,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn judges_are_fixed_after_the_judge_deadline_except_for_admins() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let ids = category_ids(&app, &owner).await;
    app.close_registration().await;
    let body = judge_body(club_id, "Anna", ADULT, true, &[(&ids[0], "technical")]);
    assert_eq!(
        add_judge(&app, &owner, body.clone()).await.status,
        StatusCode::FORBIDDEN
    );
    let admin = app.create_admin().await;
    assert_eq!(add_judge(&app, &admin, body).await.status, StatusCode::OK);
}

#[tokio::test]
async fn judges_are_validated() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let ids = category_ids(&app, &owner).await;
    let technical = [(&ids[0], "technical")];
    let dismounts = [(&ids[0], "dismounts")];

    // The first competition day is 2027-06-05: trained judges must be 15.
    let fifteen = "2012-06-05T00:00:00.000Z";
    let fourteen = "2012-06-06T00:00:00.000Z";
    let mut max_zero = judge_body(club_id, "Null", ADULT, true, &technical);
    max_zero["max_deployments"] = json!(0);
    for (body, reason) in [
        (
            judge_body(club_id, "Jung", fourteen, true, &technical),
            "under 15",
        ),
        (
            judge_body(club_id, "Ungeschult", ADULT, false, &technical),
            "untrained T",
        ),
        (
            judge_body(club_id, "=Formel", ADULT, true, &technical),
            "CSV injection",
        ),
        (
            judge_body(club_id, "  ", ADULT, true, &technical),
            "empty name",
        ),
        (max_zero, "max 0"),
        (
            judge_body(
                club_id,
                "Unbekannt",
                ADULT,
                true,
                &[(
                    &json!({ "kind": "heat", "id": Uuid::new_v4() }),
                    "technical",
                )],
            ),
            "unknown unit",
        ),
    ] {
        let response = add_judge(&app, &owner, body).await;
        assert!(
            !response.status.is_success(),
            "{reason}: {:?}",
            response.body
        );
    }

    for body in [
        judge_body(club_id, "Fünfzehn", fifteen, true, &technical),
        // Untrained dismount counters have no minimum age.
        judge_body(club_id, "Zählerin", fourteen, false, &dismounts),
    ] {
        let response = add_judge(&app, &owner, body).await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    }
    // The same name twice in one club.
    let duplicate = add_judge(
        &app,
        &owner,
        judge_body(club_id, "Fünfzehn", fifteen, true, &technical),
    )
    .await;
    assert!(!duplicate.status.is_success());
    let message = duplicate.body["message"].as_str().unwrap();
    assert!(
        message.contains("Fünfzehn Jury ist bereits gemeldet."),
        "{message}"
    );
    // Also in another case and with spaces around it.
    let duplicate = add_judge(
        &app,
        &owner,
        judge_body(club_id, " fünfzehn ", fifteen, true, &technical),
    )
    .await;
    let message = duplicate.body["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("ist bereits gemeldet."),
        "{:?}",
        duplicate.body
    );
    assert_eq!(club_judges(&app, &owner, club_id).await.len(), 2);

    // Renaming the dismount counter onto the trained judge's name, too.
    let counter = club_judges(&app, &owner, club_id)
        .await
        .into_iter()
        .find(|judge| judge["firstname"] == "Zählerin")
        .unwrap();
    let mut rename = judge_body(club_id, "FÜNFZEHN", fourteen, false, &dismounts);
    rename.as_object_mut().unwrap().remove("club_id");
    rename["judge_id"] = counter["id"].clone();
    let renamed = app
        .post("/api/command/edit_club_judge", rename, Some(&owner))
        .await;
    let message = renamed.body["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("Fünfzehn Jury ist bereits gemeldet."),
        "{:?}",
        renamed.body
    );
}

#[tokio::test]
async fn judge_endpoints_dont_exist_without_the_flag() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_jury_registration(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let list = app
        .get(
            &format!("/api/query/list_club_judges?club_id={club_id}"),
            Some(&owner),
        )
        .await;
    assert_eq!(list.status, StatusCode::NOT_FOUND);
}

// ---------- Obligation and fee (#229) ----------

async fn club(app: &TestApp, cookie: &str) -> Value {
    let response = app.get("/api/query/get_club", Some(cookie)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body
}

/// A club with `singles` single riders, so as many Küren.
async fn club_with_singles(app: &TestApp, singles: usize) -> (String, Uuid) {
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    for i in 0..singles {
        app.add_rider(
            &owner,
            common::rider_body(club_id, &format!("Fahrerin{i}"), true, false),
        )
        .await;
    }
    (owner, club_id)
}

#[tokio::test]
async fn clubs_owe_a_fee_for_missing_einsaetze() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    // 3 Küren: 2 started pairs of Küren, so 8 Einsätze.
    let (owner, club_id) = club_with_singles(&app, 3).await;
    let ids = category_ids(&app, &owner).await;

    let body = club(&app, &owner).await;
    assert_eq!(
        body["jury"],
        json!({
            "acts": 3,
            "required": 8,
            "registered": 0,
            "credited_override": null,
            "credited": 0,
            "missing": 8,
            "fee": 56.0,
        })
    );
    assert_eq!(
        body["amount_due"],
        json!({ "start_fees": 45.0, "jury_fee": 56.0, "total": 101.0, "waived": 0.0 })
    );

    // A heat (2) and a final (1); several tasks in one unit count once.
    let anna = judge_body(
        club_id,
        "Anna",
        ADULT,
        true,
        &[
            (&ids[0], "technical"),
            (&ids[0], "performance"),
            (&ids[2], "technical"),
        ],
    );
    assert_eq!(add_judge(&app, &owner, anna).await.status, StatusCode::OK);
    // Capped at 2.
    let mut berta = judge_body(
        club_id,
        "Berta",
        ADULT,
        true,
        &[(&ids[0], "technical"), (&ids[2], "performance")],
    );
    berta["max_deployments"] = json!(2);
    assert_eq!(add_judge(&app, &owner, berta).await.status, StatusCode::OK);
    // Untrained dismount counters don't count.
    let carla = judge_body(club_id, "Carla", ADULT, false, &[(&ids[2], "dismounts")]);
    assert_eq!(add_judge(&app, &owner, carla).await.status, StatusCode::OK);

    let jury = club(&app, &owner).await["jury"].clone();
    assert_eq!(jury["registered"], 5);
    assert_eq!(jury["missing"], 3);
    assert_eq!(jury["fee"], 21.0);

    // Per judge: what their units are worth, and what counts of it.
    let judges = club_judges(&app, &owner, club_id).await;
    let per_judge: Vec<Value> = judges
        .iter()
        .map(|j| {
            json!([
                j["firstname"],
                j["registered_deployments"],
                j["counted_deployments"]
            ])
        })
        .collect();
    assert_eq!(
        per_judge,
        vec![
            json!(["Anna", 3, 3]),
            json!(["Berta", 3, 2]),
            json!(["Carla", 1, 0])
        ]
    );
}

#[tokio::test]
async fn dismount_registrations_do_not_credit_mandatory_einsaetze() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = club_with_singles(&app, 1).await;
    let ids = category_ids(&app, &owner).await;

    let mut anna = judge_body(
        club_id,
        "Anna",
        ADULT,
        true,
        &[
            (&ids[0], "dismounts"),
            (&ids[1], "technical"),
            (&ids[1], "dismounts"),
            (&ids[2], "performance"),
        ],
    );
    anna["max_deployments"] = json!(1);
    assert_eq!(add_judge(&app, &owner, anna).await.status, StatusCode::OK);
    let dina = judge_body(club_id, "Dina", ADULT, true, &[(&ids[0], "dismounts")]);
    assert_eq!(add_judge(&app, &owner, dina).await.status, StatusCode::OK);

    assert_eq!(
        club(&app, &owner).await["jury"],
        json!({
            "acts": 1,
            "required": 4,
            "registered": 1,
            "credited_override": null,
            "credited": 1,
            "missing": 3,
            "fee": 21.0,
        })
    );

    let judges = club_judges(&app, &owner, club_id).await;
    assert_eq!(
        judges
            .iter()
            .map(|judge| json!([
                judge["firstname"],
                judge["registered_deployments"],
                judge["counted_deployments"],
            ]))
            .collect::<Vec<_>>(),
        vec![json!(["Anna", 5, 1]), json!(["Dina", 2, 0])]
    );
}

#[tokio::test]
async fn admins_correct_the_credited_einsaetze() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = club_with_singles(&app, 1).await;
    let admin = app.create_admin().await;
    let set = |deployments: Value| json!({ "club_id": club_id, "deployments": deployments });

    let forbidden = app
        .post(
            "/api/command/set_jury_deployments_override",
            set(json!(4)),
            Some(&owner),
        )
        .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    let response = app
        .post(
            "/api/command/set_jury_deployments_override",
            set(json!(4)),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let jury = club(&app, &owner).await["jury"].clone();
    assert_eq!(jury["credited_override"], 4);
    assert_eq!(jury["credited"], 4);
    assert_eq!(jury["fee"], 0.0);

    app.post(
        "/api/command/set_jury_deployments_override",
        set(Value::Null),
        Some(&admin),
    )
    .await;
    assert_eq!(club(&app, &owner).await["jury"]["fee"], 28.0);
}

#[tokio::test]
async fn kueren_count_like_the_start_fee() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = club_with_singles(&app, 2).await;
    let admin = app.create_admin().await;
    // An empty group costs nothing and doesn't count …
    let group = app.create_group(&owner, club_id, "Klein").await;
    assert_eq!(club(&app, &owner).await["jury"]["acts"], 2);
    // … an incomplete one (2 regular riders, no category) pays start fees
    // and counts for its owner (decided 2026-10-23).
    let riders = app.add_riders(&owner, club_id, "Gruppe", 2).await;
    app.set_group_members(&owner, club_id, group, &riders, &[])
        .await;
    assert_eq!(club(&app, &owner).await["jury"]["acts"], 3);

    // A withdrawn act counts unless its fee is waived.
    let single: Value = sqlx::query_scalar::<_, Uuid>(
        "SELECT act_id FROM act_participants JOIN acts ON acts.id = act_id WHERE act_type = 'single' LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap()
    .to_string()
    .into();
    let response = app
        .post(
            "/api/command/set_act_withdrawn",
            json!({ "act_id": single, "withdrawn": true }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(club(&app, &owner).await["jury"]["acts"], 3);
    let response = app
        .post(
            "/api/command/set_fee_waived",
            json!({ "act_id": single, "reason": "Attest" }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(club(&app, &owner).await["jury"]["acts"], 2);
}

#[tokio::test]
async fn changing_a_units_einsaetze_changes_every_club() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = club_with_singles(&app, 1).await;
    let admin = app.create_admin().await;
    let units = jury_units(&app, &admin).await;
    let anna = judge_body(club_id, "Anna", ADULT, true, &[(&units[0], "technical")]);
    assert_eq!(add_judge(&app, &owner, anna).await.status, StatusCode::OK);
    assert_eq!(club(&app, &owner).await["jury"]["registered"], 2);

    let edited = app
        .post(
            "/api/command/edit_jury_unit",
            unit_body(&units[0], 4),
            Some(&admin),
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{:?}", edited.body);
    let jury = club(&app, &owner).await["jury"].clone();
    assert_eq!(jury["registered"], 4);
    assert_eq!(jury["fee"], 0.0);
}

#[tokio::test]
async fn there_is_no_jury_fee_without_the_flag() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_jury_registration(db).await;
    let (owner, _) = club_with_singles(&app, 3).await;
    let body = club(&app, &owner).await;
    assert_eq!(body["jury"], Value::Null);
    assert_eq!(body["amount_due"]["jury_fee"], 0.0);
    assert_eq!(body["amount_due"]["total"], 45.0);
}

// ---------- Juryplan (#231) ----------

async fn plan(app: &TestApp, cookie: &str, club_id: Option<Uuid>) -> TestResponse {
    let uri = match club_id {
        Some(id) => format!("/api/query/list_jury_plan?club_id={id}"),
        None => "/api/query/list_jury_plan".to_string(),
    };
    app.get(&uri, Some(cookie)).await
}

fn assignment(judge: &Value, unit: &Value, task: &str) -> Value {
    json!({ "judge_id": judge, "unit_kind": unit["kind"], "unit_id": unit["id"], "task": task })
}

#[tokio::test]
async fn admins_assign_judges_and_publish_the_plan() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let (other, other_club) = app.create_club_owner("Other", "RSV Andere").await;
    let ids = category_ids(&app, &owner).await;
    let anna = add_judge(
        &app,
        &owner,
        judge_body(club_id, "Anna", ADULT, true, &[(&ids[0], "technical")]),
    )
    .await
    .body["judge_id"]
        .clone();
    let olga = add_judge(
        &app,
        &other,
        judge_body(other_club, "Olga", ADULT, true, &[]),
    )
    .await
    .body["judge_id"]
        .clone();

    let forbidden = app
        .post(
            "/api/command/assign_judge",
            assignment(&anna, &ids[0], "technical"),
            Some(&owner),
        )
        .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    // Also judges who didn't register for it can be assigned (the UI warns).
    for body in [
        assignment(&anna, &ids[0], "technical"),
        assignment(&olga, &ids[0], "performance"),
    ] {
        let response = app
            .post("/api/command/assign_judge", body, Some(&admin))
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    }

    let all = plan(&app, &admin, None).await;
    assert_eq!(all.status, StatusCode::OK, "{:?}", all.body);
    assert_eq!(all.body["published_at"], Value::Null);
    assert_eq!(all.body["assignments"].as_array().unwrap().len(), 2);

    // Clubs see nothing before publishing, then only their own judges.
    let own = plan(&app, &owner, Some(club_id)).await;
    assert_eq!(own.body["assignments"], json!([]));
    // The plan is published once the judges are fixed (decided 2026-10-23).
    let early = app
        .post(
            "/api/command/set_jury_plan_published",
            json!({ "published": true }),
            Some(&admin),
        )
        .await;
    assert!(!early.status.is_success(), "{:?}", early.body);
    app.close_registration().await;
    let published = app
        .post(
            "/api/command/set_jury_plan_published",
            json!({ "published": true }),
            Some(&admin),
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "{:?}", published.body);
    let own = plan(&app, &owner, Some(club_id)).await;
    assert!(own.body["published_at"].is_string());
    assert_eq!(
        own.body["assignments"],
        json!([assignment(&anna, &ids[0], "technical")])
    );
    assert_eq!(
        plan(&app, &owner, Some(other_club)).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(plan(&app, &owner, None).await.status, StatusCode::FORBIDDEN);

    // Edits after publishing show at once.
    let removed = app
        .post(
            "/api/command/unassign_judge",
            json!({ "judge_id": anna, "unit_id": ids[0]["id"] }),
            Some(&admin),
        )
        .await;
    assert_eq!(removed.status, StatusCode::OK, "{:?}", removed.body);
    assert_eq!(
        plan(&app, &owner, Some(club_id)).await.body["assignments"],
        json!([])
    );

    app.post(
        "/api/command/set_jury_plan_published",
        json!({ "published": false }),
        Some(&admin),
    )
    .await;
    assert_eq!(
        plan(&app, &admin, None).await.body["published_at"],
        Value::Null
    );
}

#[tokio::test]
async fn assignments_follow_the_jury_rules() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let ids = category_ids(&app, &owner).await;
    let anna = add_judge(&app, &owner, judge_body(club_id, "Anna", ADULT, true, &[]))
        .await
        .body["judge_id"]
        .clone();
    let carla = add_judge(
        &app,
        &owner,
        judge_body(club_id, "Carla", ADULT, false, &[]),
    )
    .await
    .body["judge_id"]
        .clone();

    // Untrained people only count dismounts.
    let untrained = app
        .post(
            "/api/command/assign_judge",
            assignment(&carla, &ids[0], "technical"),
            Some(&admin),
        )
        .await;
    assert!(!untrained.status.is_success());
    let counter = app
        .post(
            "/api/command/assign_judge",
            assignment(&carla, &ids[0], "dismounts"),
            Some(&admin),
        )
        .await;
    assert_eq!(counter.status, StatusCode::OK, "{:?}", counter.body);

    // One task per judge and unit.
    app.post(
        "/api/command/assign_judge",
        assignment(&anna, &ids[0], "technical"),
        Some(&admin),
    )
    .await;
    let second = app
        .post(
            "/api/command/assign_judge",
            assignment(&anna, &ids[0], "performance"),
            Some(&admin),
        )
        .await;
    assert!(!second.status.is_success());

    // A heat with assignments can't be deleted; a judge can.
    let refused = app
        .post(
            "/api/command/delete_heat",
            json!({ "id": ids[0]["id"] }),
            Some(&admin),
        )
        .await;
    assert!(!refused.status.is_success());
    let deleted = app
        .post(
            "/api/command/delete_club_judge",
            json!({ "judge_id": carla }),
            Some(&admin),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::OK, "{:?}", deleted.body);
    assert_eq!(
        plan(&app, &admin, None).await.body["assignments"],
        json!([assignment(&anna, &ids[0], "technical")])
    );
}

#[tokio::test]
async fn a_cross_club_group_counts_for_its_owner_club_only() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let (guest, guest_club) = app.create_club_owner("Guest", "RSV Gast").await;
    let own = app.add_riders(&owner, club_id, "Eigen", 2).await;
    let guests = app.add_riders(&guest, guest_club, "Gast", 2).await;
    let group = app.create_group(&owner, club_id, "Gemeinsam").await;
    app.set_group_members(&owner, club_id, group, &own, &[])
        .await;
    // The guest club adds one regular rider and one replacement.
    let response = app
        .set_group_members(&guest, guest_club, group, &guests[..1], &guests[1..])
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    assert_eq!(club(&app, &owner).await["jury"]["acts"], 1);
    assert_eq!(club(&app, &guest).await["jury"]["acts"], 0);
}

#[tokio::test]
async fn untrained_judges_lose_their_technical_and_performance_assignments() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let ids = category_ids(&app, &owner).await;
    let anna = add_judge(&app, &owner, judge_body(club_id, "Anna", ADULT, true, &[]))
        .await
        .body["judge_id"]
        .clone();
    for body in [
        assignment(&anna, &ids[0], "technical"),
        assignment(&anna, &ids[1], "dismounts"),
    ] {
        app.post("/api/command/assign_judge", body, Some(&admin))
            .await;
    }

    let mut edit = judge_body(club_id, "Anna", ADULT, false, &[]);
    edit.as_object_mut().unwrap().remove("club_id");
    edit["judge_id"] = anna.clone();
    let edited = app
        .post("/api/command/edit_club_judge", edit, Some(&owner))
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{:?}", edited.body);
    assert_eq!(
        plan(&app, &admin, None).await.body["assignments"],
        json!([assignment(&anna, &ids[1], "dismounts")])
    );
}
