//! A linked judge can't be newly registered for the Vorlauf of a heat in whose
//! categories the rider has an act (#405); conflicts that arise later are
//! kept and flagged.

mod common;

use axum::http::StatusCode;
use common::{TestApp, TestResponse};
use einrad_server::ids::{ActId, ClubId, RiderId};
use serde_json::{Value, json};

/// Born so that the rider is 22 on the first competition day: category EWU23
/// (single), PU23 (pair) and KG15+ (small group).
const ADULT: &str = "2005-01-01T00:00:00.000Z";
const REFUSAL: &str = "kann deshalb nicht für diesen Vorlauf gemeldet werden";

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

async fn add_heat(app: &TestApp, admin: &str, name: &str, categories: &[&str]) {
    let response = app
        .post(
            "/api/command/add_heat",
            json!({ "name": name, "description": null, "categories": categories }),
            Some(admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

/// A rider of 22 named Max Müller (or `first` Müller), with the starts of
/// `rider_body(.., single, pair)`.
async fn rider(
    app: &TestApp,
    cookie: &str,
    club_id: ClubId,
    first: &str,
    single: bool,
    pair: bool,
    partner: Option<RiderId>,
) -> RiderId {
    let mut body = common::rider_body(club_id, first, single, pair);
    body["lastname"] = json!("Müller");
    body["birthdate"] = json!(ADULT);
    body["partner_id"] = json!(partner);
    app.add_rider(cookie, body).await
}

fn judge_body(
    club_id: ClubId,
    rider_id: Option<RiderId>,
    registrations: &[(&Value, &str)],
) -> Value {
    json!({
        "club_id": club_id,
        "rider_id": rider_id,
        "firstname": "Max",
        "lastname": "Müller",
        "birthdate": ADULT,
        "is_trained": true,
        "registrations": registrations
            .iter()
            .map(|(unit, task)| json!({ "unit_kind": unit["kind"], "unit_id": unit["id"], "task": task }))
            .collect::<Vec<_>>(),
    })
}

/// Registers the rider as a linked judge for `unit` (Technik).
async fn link_and_register(
    app: &TestApp,
    cookie: &str,
    club_id: ClubId,
    rider_id: RiderId,
    unit: &Value,
) -> TestResponse {
    app.post(
        "/api/command/add_club_judge",
        judge_body(club_id, Some(rider_id), &[(unit, "technical")]),
        Some(cookie),
    )
    .await
}

fn refused(response: &TestResponse, parts: &[&str]) {
    // Refusals are `HttpError::ErrorMessages`, like every other jury refusal.
    assert!(
        !response.status.is_success(),
        "expected a refusal: {:?}",
        response.body
    );
    let text = response.body.to_string();
    assert!(text.contains(REFUSAL), "{text}");
    for part in parts {
        assert!(text.contains(part), "expected „{part}“ in {text}");
    }
}

fn ok(response: &TestResponse) {
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

async fn judge_count(app: &TestApp) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM judge")
        .fetch_one(&app.db)
        .await
        .unwrap()
}

/// An app with an admin, a club and a heat "Erwachsene" of EWU23.
async fn setup() -> (TestApp, String, String, ClubId) {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    add_heat(&app, &admin, "Erwachsene", &["EWU23"]).await;
    (app, admin, owner, club_id)
}

#[tokio::test]
async fn a_single_rider_cannot_judge_the_heat_of_their_own_category() {
    let (app, admin, owner, club_id) = setup().await;
    let max = rider(&app, &owner, club_id, "Max", true, false, None).await;
    let heat = unit(&app, &admin, "Erwachsene").await;

    let response = link_and_register(&app, &owner, club_id, max, &heat).await;
    refused(&response, &["Max Müller fährt in EWU23"]);
    assert!(!response.body.to_string().contains("Ersatzfahrer"));
    assert_eq!(judge_count(&app).await, 0);
}

#[tokio::test]
async fn pair_members_are_refused() {
    let (app, admin, owner, club_id) = setup().await;
    add_heat(&app, &admin, "Paare", &["PU23"]).await;
    let first = rider(&app, &owner, club_id, "Max", false, true, None).await;
    let second = rider(&app, &owner, club_id, "Erika", false, true, Some(first)).await;
    let heat = unit(&app, &admin, "Paare").await;

    for partner in [first, second] {
        let response = link_and_register(&app, &owner, club_id, partner, &heat).await;
        refused(&response, &["fährt in PU23"]);
    }
    assert_eq!(judge_count(&app).await, 0);
}

#[tokio::test]
async fn group_members_are_refused_and_the_replacement_is_named() {
    let (app, admin, owner, club_id) = setup().await;
    add_heat(&app, &admin, "Gruppen", &["KG15+"]).await;
    // Groups skip their Vorlauf by default, which takes no registrations.
    let unskip = app
        .post(
            "/api/command/set_heat_skip",
            json!({ "id": app.heat_id("Gruppen").await, "skip": false }),
            Some(&admin),
        )
        .await;
    ok(&unskip);
    let riders = app.add_riders(&owner, club_id, "Gruppe", 4).await;
    let group = app.create_group(&owner, club_id, "Wirbel").await;
    ok(&app
        .set_group_members(&owner, club_id, group, &riders[..3], &riders[3..])
        .await);
    let heat = unit(&app, &admin, "Gruppen").await;

    // The test riders of `add_riders` are children: make them adults.
    sqlx::query("UPDATE rider SET birthdate = ?")
        .bind("2005-01-01T00:00:00Z")
        .execute(&app.db)
        .await
        .unwrap();

    for regular in &riders[..3] {
        let response = link_and_register(&app, &owner, club_id, *regular, &heat).await;
        refused(&response, &["fährt in KG15+"]);
        assert!(!response.body.to_string().contains("Ersatzfahrer"));
    }
    let response = link_and_register(&app, &owner, club_id, riders[3], &heat).await;
    refused(&response, &["fährt in KG15+ als Ersatzfahrer"]);
    assert_eq!(judge_count(&app).await, 0);
}

#[tokio::test]
async fn a_withdrawn_act_does_not_block() {
    let (app, admin, owner, club_id) = setup().await;
    let max = rider(&app, &owner, club_id, "Max", true, false, None).await;
    let heat = unit(&app, &admin, "Erwachsene").await;
    let act: ActId = app.act_of(max).await;
    ok(&app
        .post(
            "/api/command/set_act_withdrawn",
            json!({ "act_id": act, "withdrawn": true }),
            Some(&admin),
        )
        .await);

    ok(&link_and_register(&app, &owner, club_id, max, &heat).await);
}

#[tokio::test]
async fn an_act_without_category_does_not_block() {
    let (app, admin, owner, club_id) = setup().await;
    add_heat(&app, &admin, "Gruppen", &["KG15+"]).await;
    ok(&app
        .post(
            "/api/command/set_heat_skip",
            json!({ "id": app.heat_id("Gruppen").await, "skip": false }),
            Some(&admin),
        )
        .await);
    let riders = app.add_riders(&owner, club_id, "Gruppe", 2).await;
    let group = app.create_group(&owner, club_id, "Wirbel").await;
    ok(&app
        .set_group_members(&owner, club_id, group, &riders, &[])
        .await);
    sqlx::query("UPDATE rider SET birthdate = ?")
        .bind("2005-01-01T00:00:00Z")
        .execute(&app.db)
        .await
        .unwrap();
    let heat = unit(&app, &admin, "Gruppen").await;

    // A group of two has no category.
    ok(&link_and_register(&app, &owner, club_id, riders[0], &heat).await);
}

#[tokio::test]
async fn a_rider_without_an_act_in_the_heat_may_register() {
    let (app, admin, owner, club_id) = setup().await;
    add_heat(&app, &admin, "Andere", &["EWU25"]).await;
    let max = rider(&app, &owner, club_id, "Max", true, false, None).await;
    let other = unit(&app, &admin, "Andere").await;

    ok(&link_and_register(&app, &owner, club_id, max, &other).await);
}

#[tokio::test]
async fn an_unlinked_judge_is_not_checked() {
    let (app, admin, owner, club_id) = setup().await;
    rider(&app, &owner, club_id, "Max", true, false, None).await;
    let heat = unit(&app, &admin, "Erwachsene").await;

    let response = app
        .post(
            "/api/command/add_club_judge",
            judge_body(club_id, None, &[(&heat, "technical")]),
            Some(&owner),
        )
        .await;
    ok(&response);
}

#[tokio::test]
async fn admin_and_support_are_refused_too() {
    let (app, admin, owner, club_id) = setup().await;
    let max = rider(&app, &owner, club_id, "Max", true, false, None).await;
    let heat = unit(&app, &admin, "Erwachsene").await;
    let support = app.cookie_for(app.create_user_with_roles("Support", &["support"]).await);

    for cookie in [&admin, &support] {
        let response = link_and_register(&app, cookie, club_id, max, &heat).await;
        refused(&response, &["fährt in EWU23"]);
    }
    assert_eq!(judge_count(&app).await, 0);
}

#[tokio::test]
async fn a_final_is_not_checked() {
    let (app, admin, owner, club_id) = setup().await;
    let max = rider(&app, &owner, club_id, "Max", true, false, None).await;
    let response = app
        .post(
            "/api/command/add_final",
            json!({
                "name": "Finale Erwachsene",
                "description": null,
                "act_type": "single",
                "is_single_male": false,
                "einfahrzeit_seconds": 600,
                "act_duration_seconds": 300,
                "judge_duration_seconds": 180,
                "slot_groups": [{ "heat_id": app.heat_id("Erwachsene").await, "count": 1 }],
            }),
            Some(&admin),
        )
        .await;
    ok(&response);
    let final_unit = unit(&app, &admin, "Finale Erwachsene").await;
    assert_eq!(final_unit["kind"], "final");

    // Finals are #408.
    ok(&link_and_register(&app, &owner, club_id, max, &final_unit).await);
}

#[tokio::test]
async fn an_existing_conflicting_registration_stays_but_gets_no_new_task() {
    let (app, admin, owner, club_id) = setup().await;
    let max = rider(&app, &owner, club_id, "Max", false, false, None).await;
    let heat = unit(&app, &admin, "Erwachsene").await;
    let added = link_and_register(&app, &owner, club_id, max, &heat).await;
    ok(&added);
    let judge_id = added.body["judge_id"].clone();
    // The act comes later.
    app.start_command(
        &owner,
        "set_single",
        json!({ "rider_id": max, "single": true }),
    )
    .await;

    let edit = |registrations: &[(&Value, &str)]| {
        let mut body = judge_body(club_id, Some(max), registrations);
        body["judge_id"] = judge_id.clone();
        body
    };
    let post = |body: Value| app.post("/api/command/edit_club_judge", body, Some(&owner));

    ok(&post(edit(&[(&heat, "technical")])).await);
    refused(
        &post(edit(&[(&heat, "technical"), (&heat, "performance")])).await,
        &["fährt in EWU23"],
    );
    // Unticking works.
    ok(&post(edit(&[])).await);
    // The kept registration was refused its new task; once gone, not back.
    refused(
        &post(edit(&[(&heat, "technical")])).await,
        &["fährt in EWU23"],
    );
}

#[tokio::test]
async fn a_heat_with_several_categories_names_the_category_the_rider_rides_in() {
    let (app, admin, owner, club_id) = setup().await;
    add_heat(&app, &admin, "Zwei", &["EWU25", "EWU30"]).await;
    // 26 years old on the first day: EWU30.
    let mut body = common::rider_body(club_id, "Max", true, false);
    body["lastname"] = json!("Müller");
    body["birthdate"] = json!("2001-01-01T00:00:00.000Z");
    let max = app.add_rider(&owner, body).await;
    let heat = unit(&app, &admin, "Zwei").await;

    let response = link_and_register(&app, &owner, club_id, max, &heat).await;
    refused(&response, &["fährt in EWU30"]);
    assert!(!response.body.to_string().contains("EWU25"));
}
