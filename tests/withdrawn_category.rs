//! A withdrawn act keeps the category its riders' ages give it (#346), but
//! everything that ignored withdrawn acts, because `view_act` gave them no
//! category, still ignores them. Each test names one such place.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::{Value, json};
use uuid::Uuid;

async fn withdraw(app: &TestApp, admin: &str, act_id: Uuid) {
    let response = app
        .post(
            "/api/command/set_act_withdrawn",
            json!({ "act_id": act_id, "withdrawn": true }),
            Some(admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

/// Anna and Berta, both in EWU13, and Anna withdrawn.
async fn two_acts_one_withdrawn(app: &TestApp) -> (String, Uuid, Uuid) {
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let anna = app.add_single_act(&owner, club_id, "Anna").await;
    let berta = app.add_single_act(&owner, club_id, "Berta").await;
    withdraw(app, &admin, anna).await;
    (admin, anna, berta)
}

#[tokio::test]
async fn the_view_gives_a_withdrawn_act_its_category() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (_, anna, _) = two_acts_one_withdrawn(&app).await;
    let category: Option<String> = sqlx::query_scalar("SELECT category FROM view_act WHERE id = ?")
        .bind(anna)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(category.as_deref(), Some("EWU13"));
}

#[tokio::test]
async fn a_heat_counts_no_withdrawn_acts_and_suggests_skipping_by_that_count() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    two_acts_one_withdrawn(&app).await;
    // Two acts would reach the threshold; one doesn't.
    sqlx::query("UPDATE heats SET threshold = 2 WHERE name = 'JE EW'")
        .execute(&app.db)
        .await
        .unwrap();
    let response = app.get("/api/query/list_heats", None).await;
    let heat = response
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["name"] == "JE EW")
        .unwrap();
    assert_eq!(heat["act_count"], 1);
    assert_eq!(heat["skip_suggested"], true);
}

#[tokio::test]
async fn the_timeplan_leaves_out_withdrawn_acts() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (admin, anna, berta) = two_acts_one_withdrawn(&app).await;
    let response = app
        .post(
            "/api/command/add_timeplan_entry",
            json!({
                "earliest_start_time": "2026-04-19T09:00:00Z",
                "duration_seconds": null,
                "label": null,
                "category": "EWU13",
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let response = app.get("/api/query/predict_timeplan", None).await;
    let text = response.body.to_string();
    assert!(text.contains(&berta.to_string()), "{text}");
    assert!(!text.contains(&anna.to_string()), "{text}");
}

#[tokio::test]
async fn the_startlist_and_the_acts_list_show_no_category_for_withdrawn_acts() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (admin, anna, _) = two_acts_one_withdrawn(&app).await;
    let startlist = app.get("/api/query/startlist", None).await;
    assert!(!startlist.body.to_string().contains(&anna.to_string()));
    let acts = app.get("/api/query/list_acts", Some(&admin)).await;
    let act = acts
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == anna.to_string())
        .unwrap();
    assert_eq!(act["withdrawn"], true);
    assert_eq!(act["category"], Value::Null);
    let one = app
        .get(&format!("/api/query/get_act?act_id={anna}"), Some(&admin))
        .await;
    assert_eq!(one.body["category"], Value::Null);
}

#[tokio::test]
async fn a_withdrawn_act_cannot_be_placed_in_a_final() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (admin, anna, _) = two_acts_one_withdrawn(&app).await;
    let finals = app.get("/api/query/list_finals", Some(&admin)).await;
    let slot = finals.body[0]["slots"][0]["id"].clone();
    let response = app
        .post(
            "/api/command/set_final_placement",
            json!({ "id": app.final_id("Finale EW").await, "slots": [{ "slot_id": slot, "act_id": anna }] }),
            Some(&admin),
        )
        .await;
    assert_ne!(response.status, StatusCode::OK, "{:?}", response.body);
    assert!(
        response.body.to_string().contains("zurückgezogen"),
        "{:?}",
        response.body
    );
}

#[tokio::test]
async fn a_withdrawn_group_shows_no_category_in_the_group_lists() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let admin = app.create_admin().await;
    let (owner, club_id) = app.create_club_owner("Owner", "RSV").await;
    let group = app.create_group(&owner, club_id, "Die Rollenden").await;
    let riders = app.add_riders(&owner, club_id, "G", 3).await;
    let response = app
        .set_group_members(&owner, club_id, group, &riders, &[])
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    withdraw(&app, &admin, group).await;

    let club_groups = format!("/api/query/list_club_groups?club_id={club_id}");
    for uri in ["/api/query/list_groups", club_groups.as_str()] {
        let response = app.get(uri, Some(&owner)).await;
        let found = response
            .body
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["id"] == group.to_string())
            .unwrap_or_else(|| panic!("{uri}: {:?}", response.body));
        assert_eq!(found["category"], Value::Null, "{uri}");
    }
}
