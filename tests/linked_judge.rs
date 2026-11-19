//! Deleting a rider who is linked to a judge (#384): the impact query and the
//! choice what happens to the judge.

mod common;

use axum::http::StatusCode;
use common::{TestApp, TestResponse};
use einrad_server::ids::{ClubId, RiderId};
use serde_json::{Value, json};

const BIRTHDATE: &str = "1990-05-01T00:00:00.000Z";

async fn add_rider(app: &TestApp, cookie: &str, club_id: ClubId, first: &str, last: &str) -> Value {
    let response = app
        .post(
            "/api/command/add_rider",
            json!({
                "club_id": club_id,
                "firstname": first,
                "lastname": last,
                "birthdate": BIRTHDATE,
                "gender": "female",
            }),
            Some(cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body["rider_id"].clone()
}

async fn units(app: &TestApp, cookie: &str) -> Vec<Value> {
    let response = app.get("/api/query/list_jury_units", Some(cookie)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_array().unwrap().clone()
}

/// Links `rider` to a new judge with three registrations.
async fn link_judge(app: &TestApp, cookie: &str, club_id: ClubId, rider: &Value) -> Value {
    let ids = units(app, cookie).await;
    let registrations: Vec<Value> = [(0, "technical"), (0, "performance"), (1, "technical")]
        .iter()
        .map(|(i, task)| json!({ "unit_kind": ids[*i]["kind"], "unit_id": ids[*i]["id"], "task": task }))
        .collect();
    let response = app
        .post(
            "/api/command/add_club_judge",
            json!({
                "club_id": club_id,
                "rider_id": rider,
                "is_trained": true,
                "registrations": registrations,
            }),
            Some(cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body["judge_id"].clone()
}

async fn assign(app: &TestApp, admin: &str, judge: &Value, unit: &Value, task: &str) {
    let response = app
        .post(
            "/api/command/assign_judge",
            json!({ "judge_id": judge, "unit_kind": unit["kind"], "unit_id": unit["id"], "task": task }),
            Some(admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

async fn impact(app: &TestApp, cookie: &str, rider: &Value) -> TestResponse {
    app.get(
        &format!(
            "/api/query/get_rider_delete_impact?rider_id={}",
            rider.as_str().unwrap()
        ),
        Some(cookie),
    )
    .await
}

async fn delete(app: &TestApp, cookie: &str, rider: &Value, choice: Option<&str>) -> TestResponse {
    let mut body = json!({ "rider_id": rider });
    if let Some(choice) = choice {
        body["linked_judge"] = json!(choice);
    }
    app.post("/api/command/delete_rider", body, Some(cookie))
        .await
}

async fn count(app: &TestApp, table: &str) -> i64 {
    let sql = match table {
        "rider" => "SELECT COUNT(*) FROM rider",
        "judge" => "SELECT COUNT(*) FROM judge",
        "judge_registration" => "SELECT COUNT(*) FROM judge_registration",
        "jury_assignments" => "SELECT COUNT(*) FROM jury_assignments",
        "judge_registration_unmapped" => "SELECT COUNT(*) FROM judge_registration_unmapped",
        "acts" => "SELECT COUNT(*) FROM acts",
        other => panic!("unknown table {other}"),
    };
    sqlx::query_scalar(sql).fetch_one(&app.db).await.unwrap()
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

/// A club with a linked judge (3 registrations, 2 assignments).
async fn setup(app: &TestApp) -> (String, ClubId, Value, Value, String) {
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let rider = add_rider(app, &owner, club_id, "Max", "Müller").await;
    let judge = link_judge(app, &owner, club_id, &rider).await;
    let admin = app.create_admin().await;
    let ids = units(app, &owner).await;
    assign(app, &admin, &judge, &ids[0], "technical").await;
    assign(app, &admin, &judge, &ids[1], "technical").await;
    (owner, club_id, rider, judge, admin)
}

#[tokio::test]
async fn deleting_a_rider_without_a_judge_is_unchanged() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let anna = app
        .add_rider(&owner, common::rider_body(club_id, "Anna", true, true))
        .await;
    let mut second = common::rider_body(club_id, "Berta", false, true);
    second["partner_id"] = json!(anna);
    let berta = app.add_rider(&owner, second).await;

    // A choice for a rider without a judge is ignored.
    let response = delete(&app, &owner, &json!(anna), Some("delete")).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(count(&app, "rider").await, 1);
    assert_eq!(count(&app, "judge").await, 0);
    assert_eq!(count(&app, "acts").await, 0);
    let requests = app
        .get(
            &format!("/api/query/list_club_pair_requests?club_id={club_id}"),
            Some(&owner),
        )
        .await;
    assert_eq!(
        requests.body,
        json!([{ "rider_id": berta, "partner_name": null }])
    );
}

#[tokio::test]
async fn deleting_a_linked_rider_needs_a_choice() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (owner, _club, rider, _judge, _admin) = setup(&app).await;

    let response = delete(&app, &owner, &rider, None).await;
    assert_eq!(response.status, StatusCode::CONFLICT, "{:?}", response.body);
    assert!(
        response.body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("verknüpft"),
        "{:?}",
        response.body
    );
    assert_eq!(count(&app, "rider").await, 1);
    assert_eq!(count(&app, "judge").await, 1);
}

#[tokio::test]
async fn delete_removes_the_judge_with_registrations_and_assignments() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (owner, _club, rider, judge, _admin) = setup(&app).await;

    // Unmapped registrations are deleted with the judge, so they count too.
    sqlx::query(
        "INSERT INTO judge_registration_unmapped (judge_id, old_unit, task) VALUES (?, 'Alt', 'technical')",
    )
    .bind(uuid::Uuid::parse_str(judge.as_str().unwrap()).unwrap())
    .execute(&app.db)
    .await
    .unwrap();

    let response = impact(&app, &owner, &rider).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        response.body,
        json!({ "linked_judge": {
            "judge_id": judge,
            "registrations": 4,
            "assignments": 2,
            "can_delete_judge": true,
        }})
    );

    let response = delete(&app, &owner, &rider, Some("delete")).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(count(&app, "rider").await, 0);
    assert_eq!(count(&app, "judge").await, 0);
    assert_eq!(count(&app, "judge_registration").await, 0);
    assert_eq!(count(&app, "judge_registration_unmapped").await, 0);
    assert_eq!(count(&app, "jury_assignments").await, 0);
}

#[tokio::test]
async fn keep_turns_the_judge_into_an_unlinked_copy() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (owner, club_id, rider, judge, _admin) = setup(&app).await;

    let response = delete(&app, &owner, &rider, Some("keep")).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(count(&app, "rider").await, 0);
    assert_eq!(count(&app, "judge_registration").await, 3);
    assert_eq!(count(&app, "jury_assignments").await, 2);
    let list = judges(&app, &owner, club_id).await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], judge);
    assert_eq!(list[0]["rider_id"], Value::Null);
    assert_eq!(list[0]["firstname"], "Max");
    assert_eq!(list[0]["lastname"], "Müller");
    assert_eq!(list[0]["birthdate"], "+001990-05-01T00:00:00.000000000Z");

    let edit = app
        .post(
            "/api/command/edit_club_judge",
            json!({
                "judge_id": judge,
                "firstname": "Erika",
                "lastname": "Mustermann",
                "birthdate": BIRTHDATE,
                "is_trained": true,
                "note": null,
            }),
            Some(&owner),
        )
        .await;
    assert_eq!(edit.status, StatusCode::OK, "{:?}", edit.body);
    assert_eq!(judges(&app, &owner, club_id).await[0]["firstname"], "Erika");
}

#[tokio::test]
async fn keep_with_a_name_clash_changes_nothing() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (owner, club_id, rider, judge, _admin) = setup(&app).await;
    // A typed judge, then the rider is renamed like them (allowed).
    let typed = app
        .post(
            "/api/command/add_club_judge",
            json!({
                "club_id": club_id,
                "firstname": "Petra",
                "lastname": "Beispiel",
                "birthdate": BIRTHDATE,
                "is_trained": true,
                "registrations": [],
            }),
            Some(&owner),
        )
        .await;
    assert_eq!(typed.status, StatusCode::OK, "{:?}", typed.body);
    let renamed = app
        .post(
            "/api/command/edit_rider",
            json!({
                "rider_id": rider,
                "firstname": "Petra",
                "lastname": "Beispiel",
                "birthdate": BIRTHDATE,
            }),
            Some(&owner),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{:?}", renamed.body);

    let response = delete(&app, &owner, &rider, Some("keep")).await;
    assert!(!response.status.is_success(), "{:?}", response.body);
    assert!(
        response.body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("ist bereits gemeldet."),
        "{:?}",
        response.body
    );
    assert_eq!(count(&app, "rider").await, 1);
    assert_eq!(count(&app, "judge").await, 2);
    let linked = judges(&app, &owner, club_id)
        .await
        .into_iter()
        .find(|j| j["id"] == judge)
        .unwrap();
    assert_eq!(linked["rider_id"], rider);
}

#[tokio::test]
async fn delete_choice_needs_judge_permission() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (owner, club_id, rider, _judge, admin) = setup(&app).await;
    // Judges are fixed, the riders can still change.
    sqlx::query("UPDATE competition SET jury_registration_end = ?")
        .bind(time::OffsetDateTime::now_utc() - time::Duration::days(1))
        .execute(&app.db)
        .await
        .unwrap();

    let response = impact(&app, &owner, &rider).await;
    assert_eq!(response.body["linked_judge"]["can_delete_judge"], false);

    let response = delete(&app, &owner, &rider, Some("delete")).await;
    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "{:?}",
        response.body
    );
    assert_eq!(
        response.body["message"],
        "Die Jurymeldung ist geschlossen. Die JurorIn kann nur behalten werden."
    );
    assert_eq!(count(&app, "rider").await, 1);
    assert_eq!(count(&app, "judge").await, 1);

    // Admins are always allowed.
    let response = impact(&app, &admin, &rider).await;
    assert_eq!(response.body["linked_judge"]["can_delete_judge"], true);
    let response = delete(&app, &admin, &rider, Some("delete")).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(count(&app, "judge").await, 0);

    // "keep" needs no judge permission.
    let rider = add_rider(&app, &owner, club_id, "Petra", "Beispiel").await;
    let judge = link_judge(&app, &admin, club_id, &rider).await;
    let response = delete(&app, &owner, &rider, Some("keep")).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(count(&app, "judge").await, 1);
    assert_eq!(judges(&app, &owner, club_id).await[0]["id"], judge);
}

#[tokio::test]
async fn impact_is_null_for_a_plain_rider() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let rider = add_rider(&app, &owner, club_id, "Erika", "Mustermann").await;
    let response = impact(&app, &owner, &rider).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(response.body, json!({ "linked_judge": null }));
}

#[tokio::test]
async fn only_the_club_and_admins_see_impact() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (_owner, _club, rider, _judge, admin) = setup(&app).await;
    let (other, _other_club) = app.create_club_owner("Other", "RSV Andere").await;
    assert_eq!(
        impact(&app, &other, &rider).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        impact(&app, &other, &json!(RiderId::new().to_string()))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(impact(&app, &admin, &rider).await.status, StatusCode::OK);
}
