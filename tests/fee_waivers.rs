//! Integration tests for fee waivers of clubs, riders and acts (#253).

mod common;

use axum::http::StatusCode;
use common::{TestApp, rider_body};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use uuid::Uuid;

async fn treasurer(app: &TestApp) -> String {
    app.cookie_for(app.create_user_with_roles("Kim", &["treasurer"]).await)
}

async fn waive(app: &TestApp, cookie: &str, body: Value) {
    let response = app
        .post("/api/command/set_fee_waived", body, Some(cookie))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

/// The club as the treasurer sees it in `list_clubs`.
async fn club(app: &TestApp, cookie: &str, club_id: Uuid) -> Value {
    let clubs = app.get("/api/query/list_clubs", Some(cookie)).await;
    assert_eq!(clubs.status, StatusCode::OK, "{:?}", clubs.body);
    clubs
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == club_id.to_string())
        .unwrap()
        .clone()
}

#[sqlx::test]
async fn a_waived_club_owes_nothing_but_keeps_its_jury_obligation(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Max", "RSV").await;
    app.add_single_act(&owner, club_id, "Anna").await;
    app.add_single_act(&owner, club_id, "Berta").await;
    let kasse = treasurer(&app).await;
    let before = club(&app, &kasse, club_id).await;
    // 2 starts at 15 € and a jury fee for the missing deployment.
    assert_eq!(before["amount_due"]["start_fees"], 30.0);
    assert!(before["amount_due"]["jury_fee"].as_f64().unwrap() > 0.0);

    waive(
        &app,
        &kasse,
        json!({ "club_id": club_id, "reason": "Veranstalter" }),
    )
    .await;

    let after = club(&app, &kasse, club_id).await;
    assert_eq!(after["amount_due"]["start_fees"], 0.0);
    assert_eq!(after["amount_due"]["jury_fee"], 0.0);
    assert_eq!(after["amount_due"]["total"], 0.0);
    assert_eq!(after["amount_due"]["waived"], before["amount_due"]["total"]);
    assert_eq!(after["jury"]["required"], before["jury"]["required"]);
    assert_eq!(after["fee_waiver"], "Veranstalter");
}

async fn start_fees(app: &TestApp, cookie: &str, club_id: Uuid) -> f64 {
    club(app, cookie, club_id).await["amount_due"]["start_fees"]
        .as_f64()
        .unwrap()
}

#[sqlx::test]
async fn waived_riders_cost_nothing_and_waived_acts_are_free_in_every_club(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (rsv_owner, rsv) = app.create_club_owner("Max", "RSV").await;
    let (esv_owner, esv) = app.create_club_owner("Erika", "ESV").await;
    let anna = app
        .add_rider(&rsv_owner, rider_body(rsv, "Anna", true, false))
        .await;
    let rsv_riders = app.add_riders(&rsv_owner, rsv, "R", 2).await;
    let esv_riders = app.add_riders(&esv_owner, esv, "E", 1).await;
    let group = app.create_group(&rsv_owner, rsv, "Wirbelwind").await;
    let members = app
        .set_group_members(&rsv_owner, rsv, group, &rsv_riders, &[])
        .await;
    assert_eq!(members.status, StatusCode::OK, "{:?}", members.body);
    let members = app
        .set_group_members(&esv_owner, esv, group, &esv_riders, &[])
        .await;
    assert_eq!(members.status, StatusCode::OK, "{:?}", members.body);
    let kasse = treasurer(&app).await;
    // Anna's single and the group starts of R1, R2 and E1, at 15 € each.
    assert_eq!(start_fees(&app, &kasse, rsv).await, 45.0);
    assert_eq!(start_fees(&app, &kasse, esv).await, 15.0);

    waive(
        &app,
        &kasse,
        json!({ "act_id": group, "reason": "befreundeter Verein" }),
    )
    .await;
    assert_eq!(start_fees(&app, &kasse, rsv).await, 15.0);
    assert_eq!(start_fees(&app, &kasse, esv).await, 0.0);
    assert_eq!(club(&app, &kasse, esv).await["amount_due"]["waived"], 15.0);

    waive(&app, &kasse, json!({ "rider_id": anna, "reason": "Gast" })).await;
    assert_eq!(start_fees(&app, &kasse, rsv).await, 0.0);

    let riders = app.get("/api/query/list_riders", Some(&kasse)).await;
    let anna = riders
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["firstname"] == "Anna")
        .unwrap();
    assert_eq!(anna["fee_waived"], true);
    assert_eq!(anna["fee_waiver"], "Gast");
    let r1 = riders
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["firstname"] == "R1")
        .unwrap();
    assert_eq!(r1["waived_count"], 1);
}

#[sqlx::test]
async fn the_cap_applies_to_the_starts_left_after_act_waivers(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, rsv) = app.create_club_owner("Max", "RSV").await;
    let dora = app
        .add_rider(&owner, rider_body(rsv, "Dora", false, true))
        .await;
    let mut clara = rider_body(rsv, "Clara", true, true);
    clara["partner_id"] = json!(dora);
    let clara = app.add_rider(&owner, clara).await;
    let small = app.create_group(&owner, rsv, "Klein").await;
    let large = app.create_large_group(&owner, rsv, "Groß").await;
    for group in [small, large] {
        let members = app
            .set_group_members(&owner, rsv, group, &[clara], &[])
            .await;
        assert_eq!(members.status, StatusCode::OK, "{:?}", members.body);
    }
    let kasse = treasurer(&app).await;
    // Clara: 4 starts, capped at 45 €; Dora: the pair, 15 €.
    assert_eq!(start_fees(&app, &kasse, rsv).await, 60.0);

    waive(&app, &kasse, json!({ "act_id": large, "reason": "Attest" })).await;
    assert_eq!(start_fees(&app, &kasse, rsv).await, 60.0);

    waive(&app, &kasse, json!({ "act_id": small, "reason": "Attest" })).await;
    assert_eq!(start_fees(&app, &kasse, rsv).await, 45.0);
}

#[sqlx::test]
async fn only_a_withdrawn_waived_act_drops_out_of_the_jury_obligation(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, rsv) = app.create_club_owner("Max", "RSV").await;
    let anna = app.add_single_act(&owner, rsv, "Anna").await;
    app.add_single_act(&owner, rsv, "Berta").await;
    app.add_single_act(&owner, rsv, "Clara").await;
    let kasse = treasurer(&app).await;
    let admin = app.create_admin().await;
    let acts = |club: Value| club["jury"]["acts"].clone();
    assert_eq!(acts(club(&app, &kasse, rsv).await), 3);

    waive(&app, &kasse, json!({ "act_id": anna, "reason": "Attest" })).await;
    assert_eq!(acts(club(&app, &kasse, rsv).await), 3);

    let withdraw = |withdrawn: bool| json!({ "act_id": anna, "withdrawn": withdrawn });
    let response = app
        .post(
            "/api/command/set_act_withdrawn",
            withdraw(true),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(acts(club(&app, &kasse, rsv).await), 2);

    // Taking the withdrawal back keeps the waiver.
    let response = app
        .post(
            "/api/command/set_act_withdrawn",
            withdraw(false),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let act = app
        .get(&format!("/api/query/get_act?act_id={anna}"), Some(&kasse))
        .await;
    assert_eq!(act.body["fee_waived"], true);
    assert_eq!(act.body["fee_waiver"], "Attest");
    assert_eq!(acts(club(&app, &kasse, rsv).await), 3);
}

#[sqlx::test]
async fn granting_needs_a_reason_revoking_doesnt(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, rsv) = app.create_club_owner("Max", "RSV").await;
    app.add_single_act(&owner, rsv, "Anna").await;
    let kasse = treasurer(&app).await;

    for reason in [json!(""), json!("   ")] {
        let response = app
            .post(
                "/api/command/set_fee_waived",
                json!({ "club_id": rsv, "reason": reason }),
                Some(&kasse),
            )
            .await;
        assert_ne!(response.status, StatusCode::OK, "{reason}");
    }
    let both = app
        .post(
            "/api/command/set_fee_waived",
            json!({ "club_id": rsv, "act_id": Uuid::now_v7(), "reason": "x" }),
            Some(&kasse),
        )
        .await;
    assert_ne!(both.status, StatusCode::OK);
    let unknown = app
        .post(
            "/api/command/set_fee_waived",
            json!({ "rider_id": Uuid::now_v7(), "reason": "x" }),
            Some(&kasse),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    assert_eq!(start_fees(&app, &kasse, rsv).await, 15.0);

    waive(
        &app,
        &kasse,
        json!({ "club_id": rsv, "reason": "Veranstalter" }),
    )
    .await;
    assert_eq!(start_fees(&app, &kasse, rsv).await, 0.0);
    // The admin may revoke (and grant) as well.
    let admin = app.create_admin().await;
    waive(&app, &admin, json!({ "club_id": rsv, "reason": null })).await;
    assert_eq!(start_fees(&app, &kasse, rsv).await, 15.0);
    assert_eq!(club(&app, &kasse, rsv).await["fee_waiver"], Value::Null);
}

#[sqlx::test]
async fn each_grant_and_revoke_writes_one_log_entry(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (rsv_owner, rsv) = app.create_club_owner("Max", "RSV").await;
    let (esv_owner, esv) = app.create_club_owner("Erika", "ESV").await;
    let anna = app
        .add_rider(&rsv_owner, rider_body(rsv, "Anna", true, false))
        .await;
    let rsv_riders = app.add_riders(&rsv_owner, rsv, "R", 2).await;
    let esv_riders = app.add_riders(&esv_owner, esv, "E", 1).await;
    let group = app.create_group(&rsv_owner, rsv, "Wirbelwind").await;
    app.set_group_members(&rsv_owner, rsv, group, &rsv_riders, &[])
        .await;
    app.set_group_members(&esv_owner, esv, group, &esv_riders, &[])
        .await;
    let kim = app.create_user_with_roles("Kim", &["treasurer"]).await;
    let kasse = app.cookie_for(kim);

    waive(
        &app,
        &kasse,
        json!({ "club_id": rsv, "reason": "Veranstalter" }),
    )
    .await;
    // The same reason again changes nothing to log.
    waive(
        &app,
        &kasse,
        json!({ "club_id": rsv, "reason": "Veranstalter" }),
    )
    .await;
    waive(&app, &kasse, json!({ "rider_id": anna, "reason": "Gast" })).await;
    waive(
        &app,
        &kasse,
        json!({ "act_id": group, "reason": "befreundeter Verein" }),
    )
    .await;
    waive(&app, &kasse, json!({ "club_id": rsv, "reason": null })).await;

    let log = app.get("/api/query/list_booking_log", Some(&kasse)).await;
    assert_eq!(log.status, StatusCode::OK, "{:?}", log.body);
    let mut entries = log.body.as_array().unwrap().clone();
    entries.reverse();
    let summary: Vec<_> = entries
        .iter()
        .map(|e| {
            (
                e["action"].as_str().unwrap(),
                e["waiver"]["level"].as_str().unwrap(),
                e["waiver"]["name"].as_str().unwrap(),
                e["waiver"]["reason"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("waived", "club", "RSV", "Veranstalter"),
            ("waived", "rider", "Anna Muster", "Gast"),
            ("waived", "act", "Wirbelwind", "befreundeter Verein"),
            ("waiver_revoked", "club", "RSV", "Veranstalter"),
        ]
    );
    for entry in &entries {
        assert_eq!(entry["user_id"], kim.to_string());
        assert_eq!(entry["roles"], json!(["treasurer"]));
        assert_eq!(entry["booking_id"], Value::Null);
    }

    // The act waiver concerns both clubs of its riders.
    let esv_log = app
        .get(
            &format!("/api/query/list_booking_log?club_id={esv}"),
            Some(&kasse),
        )
        .await;
    let actions: Vec<_> = esv_log
        .body
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["waiver"]["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(actions, ["Wirbelwind"]);
}

#[sqlx::test]
async fn clubs_see_what_is_waived_but_not_why(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (owner, rsv) = app.create_club_owner("Max", "RSV").await;
    let anna = app
        .add_rider(&owner, rider_body(rsv, "Anna", true, false))
        .await;
    let act = app.act_of(anna).await;
    let kasse = treasurer(&app).await;
    let desk = app.cookie_for(app.create_user_with_roles("Desk", &["check_in"]).await);
    waive(&app, &kasse, json!({ "act_id": act, "reason": "Attest" })).await;

    for cookie in [&owner, &desk] {
        let forbidden = app
            .post(
                "/api/command/set_fee_waived",
                json!({ "club_id": rsv, "reason": "x" }),
                Some(cookie),
            )
            .await;
        assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    }

    let acts = app
        .get(
            &format!("/api/query/list_club_acts?club_id={rsv}"),
            Some(&owner),
        )
        .await;
    assert_eq!(acts.body[0]["fee_waived"], true, "{:?}", acts.body);
    assert!(!acts.body.to_string().contains("Attest"));
    let public = app
        .get(&format!("/api/query/get_act?act_id={act}"), None)
        .await;
    assert_eq!(public.body["fee_waived"], true);
    assert_eq!(public.body["fee_waiver"], Value::Null);

    waive(
        &app,
        &kasse,
        json!({ "club_id": rsv, "reason": "Veranstalter" }),
    )
    .await;
    let own = app.get("/api/query/get_club", Some(&owner)).await;
    assert_eq!(own.body["fee_waived"], true);
    assert!(!own.body.to_string().contains("Veranstalter"));
    let log = app.get("/api/query/list_booking_log", Some(&owner)).await;
    assert_eq!(log.status, StatusCode::FORBIDDEN);
}

/// The migration that replaced `acts.fee_waived` with waiver reasons.
const FEE_WAIVERS_MIGRATION: i64 = 20261026120000;

#[sqlx::test(migrations = false)]
async fn the_migration_keeps_waived_acts_with_a_reason(db: SqlitePool) {
    let migrator = sqlx::migrate!();
    migrator
        .run_to(FEE_WAIVERS_MIGRATION - 1, &db)
        .await
        .unwrap();
    let owner = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, email, email_verified, name, password) VALUES (?, 'max@cup.test', true, 'Max', 'x')",
    )
    .bind(owner)
    .execute(&db)
    .await
    .unwrap();
    let rsv = Uuid::now_v7();
    sqlx::query("INSERT INTO clubs (id, name, owner_id) VALUES (?, 'RSV', ?)")
        .bind(rsv)
        .bind(owner)
        .execute(&db)
        .await
        .unwrap();
    let mut acts = Vec::new();
    // The schema before the fee waivers still calls riders starters.
    for (firstname, waived) in [("Anna", true), ("Berta", false)] {
        let rider = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO starter (id, firstname, lastname, birthdate, club_id, single_male, single_female, pair) VALUES (?, ?, 'Muster', '2010-01-01', ?, false, true, false)",
        )
        .bind(rider)
        .bind(firstname)
        .bind(rsv)
        .execute(&db)
        .await
        .unwrap();
        let act = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO acts (id, name, act_type, withdrawn_at, fee_waived) VALUES (?, '', 'single', datetime('now'), ?)",
        )
        .bind(act)
        .bind(waived)
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO act_participants (act_id, starter_id) VALUES (?, ?)")
            .bind(act)
            .bind(rider)
            .execute(&db)
            .await
            .unwrap();
        acts.push(act);
    }
    migrator.run(&db).await.unwrap();
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;

    let anna = app
        .get(
            &format!("/api/query/get_act?act_id={}", acts[0]),
            Some(&admin),
        )
        .await;
    assert_eq!(anna.body["fee_waived"], true, "{:?}", anna.body);
    assert_eq!(anna.body["fee_waiver"], "Abmeldung (übernommen)");
    let berta = app
        .get(
            &format!("/api/query/get_act?act_id={}", acts[1]),
            Some(&admin),
        )
        .await;
    assert_eq!(berta.body["fee_waived"], false);

    let log = app.get("/api/query/list_booking_log", Some(&admin)).await;
    let entries = log.body.as_array().unwrap();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["action"], "taken_over");
    assert_eq!(entries[0]["user_id"], Value::Null);
    assert_eq!(entries[0]["waiver"]["level"], "act");
    assert_eq!(entries[0]["waiver"]["target_id"], acts[0].to_string());
    assert_eq!(entries[0]["waiver"]["name"], "Anna Muster");
    assert_eq!(
        entries[0]["waiver"]["clubs"],
        json!([{ "id": rsv, "name": "RSV" }])
    );
}
