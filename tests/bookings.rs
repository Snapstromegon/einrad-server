//! Integration tests for bookings (#248) and the payment log (#249).

mod common;

use axum::http::StatusCode;
use common::TestApp;
use einrad_server::ids::{BookingId, ClubId, UserId};
use serde_json::{Value, json};
use uuid::Uuid;

/// The migration that replaced `clubs.payment` with bookings.
const BOOKINGS_MIGRATION: i64 = 20261024120000;

fn booking(club_id: Option<ClubId>, amount: f64) -> Value {
    json!({ "club_id": club_id, "amount": amount, "date": "2027-03-01", "note": "Überweisung" })
}

async fn create(app: &TestApp, cookie: &str, body: Value) -> BookingId {
    let response = app
        .post("/api/command/create_booking", body, Some(cookie))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body["id"].as_str().unwrap().parse().unwrap()
}

async fn edit(app: &TestApp, cookie: &str, booking_id: BookingId, booking: Value) {
    let response = app
        .post(
            "/api/command/edit_booking",
            json!({ "booking_id": booking_id, "booking": booking }),
            Some(cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

async fn delete(app: &TestApp, cookie: &str, booking_id: BookingId) {
    let response = app
        .post(
            "/api/command/delete_booking",
            json!({ "booking_id": booking_id }),
            Some(cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
}

/// The paid amount of every club, by name, as the treasurer sees it.
async fn paid(app: &TestApp, cookie: &str) -> Vec<(String, f64)> {
    let clubs = app.get("/api/query/list_clubs", Some(cookie)).await;
    assert_eq!(clubs.status, StatusCode::OK, "{:?}", clubs.body);
    clubs
        .body
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["name"].as_str().unwrap().to_string(),
                c["paid"].as_f64().unwrap(),
            )
        })
        .collect()
}

async fn log(app: &TestApp, cookie: &str, query: &str) -> Vec<Value> {
    let response = app
        .get(&format!("/api/query/list_booking_log{query}"), Some(cookie))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_array().unwrap().clone()
}

async fn treasurer(app: &TestApp) -> (UserId, String) {
    let id = app.create_user_with_roles("Kim", &["treasurer"]).await;
    (id, app.cookie_for(id))
}

#[tokio::test]
async fn a_payment_and_a_refund_add_up_to_the_paid_amount() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Max", "RSV").await;
    let (_, kasse) = treasurer(&app).await;

    create(&app, &kasse, booking(Some(club_id), 120.5)).await;
    create(&app, &kasse, booking(Some(club_id), -20.3)).await;

    assert_eq!(paid(&app, &kasse).await, [("RSV".into(), 100.2)]);
    let club = app.get("/api/query/get_club", Some(&owner)).await;
    assert_eq!(club.body["paid"], 100.2);
    let amounts: Vec<_> = club.body["payments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["amount"].as_f64().unwrap(), p["note"].as_str().unwrap()))
        .collect();
    assert_eq!(amounts, [(120.5, "Überweisung"), (-20.3, "Überweisung")]);
}

#[tokio::test]
async fn unassigned_bookings_count_once_assigned_ignored_ones_never() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (_, club_id) = app.create_club_owner("Max", "RSV").await;
    let (_, kasse) = treasurer(&app).await;

    let transfer = create(&app, &kasse, booking(None, 45.0)).await;
    let mut fees = booking(None, -4.9);
    fees["ignored"] = json!(true);
    let fees = create(&app, &kasse, fees).await;
    assert_eq!(paid(&app, &kasse).await, [("RSV".into(), 0.0)]);

    edit(&app, &kasse, transfer, booking(Some(club_id), 45.0)).await;
    assert_eq!(paid(&app, &kasse).await, [("RSV".into(), 45.0)]);

    let bookings = app.get("/api/query/list_bookings", Some(&kasse)).await;
    let ignored = bookings
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == fees.to_string())
        .unwrap();
    assert_eq!(ignored["ignored"], true);
    assert_eq!(ignored["club_id"], Value::Null);
    let transfer = bookings
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == transfer.to_string())
        .unwrap();
    assert_eq!(transfer["club_name"], "RSV");
}

#[tokio::test]
async fn editing_and_deleting_change_the_paid_amount() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (_, club_id) = app.create_club_owner("Max", "RSV").await;
    let (_, kasse) = treasurer(&app).await;
    let first = create(&app, &kasse, booking(Some(club_id), 50.0)).await;
    let second = create(&app, &kasse, booking(Some(club_id), 30.0)).await;

    edit(&app, &kasse, first, booking(Some(club_id), 60.0)).await;
    assert_eq!(paid(&app, &kasse).await, [("RSV".into(), 90.0)]);

    delete(&app, &kasse, second).await;
    assert_eq!(paid(&app, &kasse).await, [("RSV".into(), 60.0)]);
}

#[tokio::test]
async fn invalid_bookings_are_refused() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (_, club_id) = app.create_club_owner("Max", "RSV").await;
    let (_, kasse) = treasurer(&app).await;
    let mut ignored_payment = booking(Some(club_id), 10.0);
    ignored_payment["ignored"] = json!(true);

    for (body, expected) in [
        // Message errors come back as 500 with the message, as elsewhere.
        (
            booking(Some(club_id), 0.0),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
        (ignored_payment, StatusCode::INTERNAL_SERVER_ERROR),
        (booking(Some(ClubId::new()), 10.0), StatusCode::NOT_FOUND),
    ] {
        let response = app
            .post("/api/command/create_booking", body.clone(), Some(&kasse))
            .await;
        assert_eq!(response.status, expected, "{body}: {:?}", response.body);
    }
    let missing = app
        .post(
            "/api/command/delete_booking",
            json!({ "booking_id": Uuid::now_v7() }),
            Some(&kasse),
        )
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert!(log(&app, &kasse, "").await.is_empty());
}

#[tokio::test]
async fn only_treasurer_and_admin_write_bookings() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Max", "RSV").await;
    let (_, kasse) = treasurer(&app).await;
    let admin = app.create_admin().await;
    let id = create(&app, &kasse, booking(Some(club_id), 10.0)).await;
    create(&app, &admin, booking(Some(club_id), 10.0)).await;

    let others: [&[&str]; 4] = [&[], &["check_in"], &["jury_lead"], &["music_control"]];
    for (i, roles) in others.into_iter().enumerate() {
        let name = format!("Alex{i}");
        let cookie = app.cookie_for(app.create_user_with_roles(&name, roles).await);
        for cookie in [cookie.as_str(), owner.as_str()] {
            for (uri, body) in [
                ("/api/command/create_booking", booking(Some(club_id), 5.0)),
                (
                    "/api/command/edit_booking",
                    json!({ "booking_id": id, "booking": booking(None, 5.0) }),
                ),
                ("/api/command/delete_booking", json!({ "booking_id": id })),
            ] {
                let response = app.post(uri, body, Some(cookie)).await;
                assert_eq!(response.status, StatusCode::FORBIDDEN, "{uri} {roles:?}");
            }
            let bookings = app.get("/api/query/list_bookings", Some(cookie)).await;
            assert_eq!(bookings.status, StatusCode::FORBIDDEN, "{roles:?}");
        }
    }
    assert_eq!(paid(&app, &kasse).await, [("RSV".into(), 20.0)]);
}

#[tokio::test]
async fn a_club_sees_only_its_own_payments() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, _) = app.create_club_owner("Max", "RSV").await;
    let (other, other_id) = app.create_club_owner("Erika", "ESV").await;
    let (_, kasse) = treasurer(&app).await;
    create(&app, &kasse, booking(Some(other_id), 30.0)).await;

    let foreign = app
        .get(
            &format!("/api/query/get_club?club_id={other_id}"),
            Some(&owner),
        )
        .await;
    assert_eq!(foreign.status, StatusCode::OK, "{:?}", foreign.body);
    assert_eq!(foreign.body["paid"], Value::Null);
    assert_eq!(foreign.body["payments"], Value::Null);
    let anonymous = app
        .get(&format!("/api/query/get_club?club_id={other_id}"), None)
        .await;
    assert_eq!(anonymous.body["paid"], Value::Null);

    let own = app.get("/api/query/get_club", Some(&other)).await;
    assert_eq!(own.body["paid"], 30.0);
    assert_eq!(own.body["payments"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_migration_turns_paid_totals_into_bookings() {
    let db = common::in_memory().await;
    let migrator = &db_sqlite::MIGRATOR;
    migrator.run_to(BOOKINGS_MIGRATION - 1, &db).await.unwrap();
    let owner = UserId::new();
    sqlx::query(
        "INSERT INTO users (id, email, email_verified, name, password) VALUES ($1, 'max@cup.test', true, 'Max', 'x')",
    )
    .bind(owner)
    .execute(&db)
    .await
    .unwrap();
    let rsv = ClubId::new();
    for (name, payment) in [("RSV", Some(120.5)), ("ESV", Some(0.0)), ("SSV", None)] {
        let id = if name == "RSV" { rsv } else { ClubId::new() };
        sqlx::query("INSERT INTO clubs (id, name, owner_id, payment) VALUES ($1, $2, $3, $4)")
            .bind(id)
            .bind(name)
            .bind(owner)
            .bind(payment)
            .execute(&db)
            .await
            .unwrap();
    }
    migrator.run(&db).await.unwrap();
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;

    let bookings = app.get("/api/query/list_bookings", Some(&admin)).await;
    let bookings = bookings.body.as_array().unwrap();
    assert_eq!(bookings.len(), 1, "{bookings:?}");
    assert_eq!(bookings[0]["club_id"], rsv.to_string());
    assert_eq!(bookings[0]["amount"], 120.5);
    assert_eq!(bookings[0]["note"], "Übernahme bisheriger Zahlungen");
    assert_eq!(
        paid(&app, &admin).await,
        [
            ("ESV".into(), 0.0),
            ("RSV".into(), 120.5),
            ("SSV".into(), 0.0)
        ]
    );

    let entries = log(&app, &admin, &format!("?club_id={rsv}")).await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["action"], "taken_over");
    assert_eq!(entries[0]["user_id"], Value::Null);
    assert_eq!(entries[0]["before"], Value::Null);
    assert_eq!(entries[0]["after"]["amount"], 120.5);
    assert_eq!(entries[0]["after"]["club_name"], "RSV");
    assert_eq!(entries[0]["booking_id"], bookings[0]["id"]);
}

#[tokio::test]
async fn each_change_writes_one_log_entry_with_before_and_after() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (_, rsv) = app.create_club_owner("Max", "RSV").await;
    let (_, esv) = app.create_club_owner("Erika", "ESV").await;
    let (kim, kasse) = treasurer(&app).await;

    let id = create(&app, &kasse, booking(None, 45.0)).await;
    edit(&app, &kasse, id, booking(Some(rsv), 45.0)).await;
    edit(&app, &kasse, id, booking(Some(esv), 45.0)).await;
    edit(&app, &kasse, id, booking(Some(esv), 50.0)).await;
    edit(&app, &kasse, id, booking(None, 50.0)).await;
    let mut ignored = booking(None, 50.0);
    ignored["ignored"] = json!(true);
    edit(&app, &kasse, id, ignored.clone()).await;
    // Unchanged: nothing to log.
    edit(&app, &kasse, id, ignored).await;
    delete(&app, &kasse, id).await;

    let mut entries = log(&app, &kasse, "").await;
    entries.reverse();
    let actions: Vec<_> = entries
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        actions,
        [
            "created",
            "assigned",
            "assigned",
            "edited",
            "unassigned",
            "ignored",
            "deleted"
        ]
    );
    for entry in &entries {
        assert_eq!(entry["booking_id"], id.to_string());
        assert_eq!(entry["user_id"], kim.to_string());
        assert_eq!(entry["user_name"], "Kim");
        assert_eq!(entry["roles"], json!(["treasurer"]));
    }
    let state = |b: &Value| {
        (
            b["amount"].as_f64().unwrap(),
            b["club_name"].as_str().map(str::to_string),
            b["ignored"].as_bool().unwrap(),
        )
    };
    assert_eq!(entries[0]["before"], Value::Null);
    assert_eq!(state(&entries[0]["after"]), (45.0, None, false));
    assert_eq!(
        state(&entries[2]["before"]),
        (45.0, Some("RSV".into()), false)
    );
    assert_eq!(
        state(&entries[2]["after"]),
        (45.0, Some("ESV".into()), false)
    );
    assert_eq!(
        state(&entries[3]["before"]),
        (45.0, Some("ESV".into()), false)
    );
    assert_eq!(
        state(&entries[3]["after"]),
        (50.0, Some("ESV".into()), false)
    );
    assert_eq!(state(&entries[5]["after"]), (50.0, None, true));
    assert_eq!(state(&entries[6]["before"]), (50.0, None, true));
    assert_eq!(entries[6]["after"], Value::Null);

    // Per club: the changes the club's payment was part of.
    let rsv_actions: Vec<_> = log(&app, &kasse, &format!("?club_id={rsv}"))
        .await
        .iter()
        .map(|e| e["action"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(rsv_actions, ["assigned", "assigned"]);
}

#[tokio::test]
async fn only_treasurer_and_admin_read_the_log() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Max", "RSV").await;
    let (_, kasse) = treasurer(&app).await;
    create(&app, &kasse, booking(Some(club_id), 10.0)).await;
    let admin = app.create_admin().await;

    assert_eq!(log(&app, &admin, "").await.len(), 1);
    assert_eq!(log(&app, &kasse, "").await.len(), 1);
    for roles in [&["check_in"][..], &["jury_lead"]] {
        let cookie = app.cookie_for(app.create_user_with_roles(roles[0], roles).await);
        let response = app.get("/api/query/list_booking_log", Some(&cookie)).await;
        assert_eq!(response.status, StatusCode::FORBIDDEN, "{roles:?}");
    }
    let club = app
        .get(
            &format!("/api/query/list_booking_log?club_id={club_id}"),
            Some(&owner),
        )
        .await;
    assert_eq!(club.status, StatusCode::FORBIDDEN);
    let anonymous = app.get("/api/query/list_booking_log", None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
}
