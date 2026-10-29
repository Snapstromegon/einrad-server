//! `GET /metrics` (Prometheus).

mod common;

use axum::http::StatusCode;
use common::{METRICS_TOKEN, TestApp, rider_body};
use serde_json::json;
use sqlx::SqlitePool;

async fn scrape(app: &TestApp) -> String {
    let response = app.get_with_bearer("/metrics", METRICS_TOKEN).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert!(
        response.headers[axum::http::header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("application/openmetrics-text")
    );
    response.body.as_str().unwrap().to_string()
}

fn has_line(body: &str, line: &str) -> bool {
    body.lines().any(|l| l == line)
}

#[sqlx::test]
async fn metrics_need_the_token(db: SqlitePool) {
    let app = TestApp::new(db).await;

    let missing = app.get("/metrics", None).await;
    assert_eq!(missing.status, StatusCode::UNAUTHORIZED);
    let wrong = app.get_with_bearer("/metrics", "wrong").await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    // An admin login isn't enough either.
    let admin = app.create_admin().await;
    let as_admin = app.get("/metrics", Some(&admin)).await;
    assert_eq!(as_admin.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn metrics_count_registrations_acts_and_money(db: SqlitePool) {
    // Start fees only; with the jury registration the jury fee adds to it.
    let app = TestApp::without_jury_registration(db).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    app.add_single_act(&cookie, club_id, "Anna").await;
    let berta = app
        .add_rider(&cookie, rider_body(club_id, "Berta", false, true))
        .await;
    let mut carla = rider_body(club_id, "Carla", true, true);
    carla["partner_id"] = json!(berta);
    app.add_rider(&cookie, carla).await;
    let admin = app.create_admin().await;
    app.post(
        "/api/command/create_booking",
        json!({ "club_id": club_id, "amount": 20.0, "date": "2027-03-01" }),
        Some(&admin),
    )
    .await;

    let body = scrape(&app).await;

    assert!(has_line(&body, "cup_clubs 1"), "{body}");
    assert!(has_line(&body, "cup_riders 3"), "{body}");
    assert!(
        has_line(&body, r#"cup_riders_by_club{club="RSV"} 3"#),
        "{body}"
    );
    assert!(
        has_line(
            &body,
            r#"cup_acts{act_type="single",state="categorized"} 2"#
        ),
        "{body}"
    );
    assert!(
        has_line(&body, r#"cup_acts{act_type="pair",state="categorized"} 1"#),
        "{body}"
    );
    assert!(has_line(&body, r#"cup_songs{state="missing"} 3"#), "{body}");
    assert!(has_line(&body, "cup_registration_open 1"), "{body}");
    assert!(
        has_line(&body, "cup_payments_received_euros 20.0"),
        "{body}"
    );
    // Anna 1 start, Berta 1 (pair), Carla 2 (single and pair): 4 × 15 €.
    assert!(has_line(&body, "cup_fees_due_euros 60.0"), "{body}");
    // Requests are counted by their matched route.
    assert!(
        body.contains(r#"cup_http_requests_total{method="POST",route="/api/command/create_booking",status="200"}"#),
        "{body}"
    );
    assert!(body.contains("cup_build_info{version="), "{body}");
    assert!(body.ends_with("# EOF\n"), "{body}");
}

#[sqlx::test]
async fn metrics_show_the_live_timeplan(db: SqlitePool) {
    let app = TestApp::new(db).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    app.add_single_act(&cookie, club_id, "Anna").await;
    let admin = app.create_admin().await;
    for _ in 0..3 {
        app.post("/api/command/timeplan_forward", json!(null), Some(&admin))
            .await;
    }

    let body = scrape(&app).await;

    assert!(has_line(&body, "cup_acts_performed 1"), "{body}");
    assert!(body.contains("cup_timeplan_delay_seconds "), "{body}");
    assert!(
        body.contains(r#"cup_timeplan_entries{status="ended"} "#),
        "{body}"
    );
    assert!(
        body.contains("cup_timeplan_predicted_end_timestamp_seconds "),
        "{body}"
    );
}
