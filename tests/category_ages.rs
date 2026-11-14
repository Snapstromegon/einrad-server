//! Integration tests for categories defined by age classes (#256): the
//! birthday bounds are derived from the determining day, never stored.

mod common;

use axum::http::StatusCode;
use common::{TestApp, rider_body};
use einrad_server::ids::ClubId;
use serde_json::{Value, json};
use sqlx::SqlitePool;

/// The last migration before the categories store ages.
const BEFORE_AGES: i64 = 20261109130000;

async fn migrated_before_ages() -> SqlitePool {
    let db = common::in_memory().await;
    sqlx::migrate!("./migrations")
        .run_to(BEFORE_AGES, &db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO competition (id, name, location, start_date, end_date, determining_day, \
         registration_start, registration_end, music_upload_end, fee_per_start) \
         VALUES ('cup', 'Cup', 'Halle', '2027-06-05', '2027-06-06', '2027-06-01', \
         '2026-11-01T00:00:00+01:00', '2027-04-30T23:59:59+02:00', '2027-05-20T23:59:59+02:00', 15)",
    )
    .execute(&db)
    .await
    .unwrap();
    db
}

/// Inserts a category with stored birthday bounds, as before the ages.
async fn insert_dated(db: &SqlitePool, name: &str, from: Option<&str>, to: Option<&str>) {
    sqlx::query(
        "INSERT INTO categories (name, from_birthday, to_birthday, act_type) \
         VALUES ($1, $2, $3, 'single')",
    )
    .bind(name)
    .bind(from)
    .bind(to)
    .execute(db)
    .await
    .unwrap();
}

async fn stored_ages(db: &SqlitePool) -> Vec<(String, Option<f64>, Option<f64>)> {
    sqlx::query_as("SELECT name, min_age, under_age FROM categories ORDER BY name")
        .fetch_all(db)
        .await
        .unwrap()
}

#[tokio::test]
async fn the_migration_turns_stored_birthday_bounds_into_ages() {
    let db = migrated_before_ages().await;
    // As the import wrote them (RFC 3339) and as the category form sent them.
    insert_dated(
        &db,
        "EWU11",
        Some("2016-06-01T00:00:00Z"),
        Some("9999-12-31T00:00:00Z"),
    )
    .await;
    insert_dated(&db, "EWU13", Some("2014-06-01"), Some("2016-06-01")).await;
    insert_dated(&db, "U9,5", Some("2017-12-01T00:00:00Z"), None).await;
    insert_dated(
        &db,
        "EW70+",
        Some("1900-01-01T00:00:00Z"),
        Some("1957-06-01T00:00:00Z"),
    )
    .await;

    sqlx::migrate!("./migrations").run(&db).await.unwrap();

    assert_eq!(
        stored_ages(&db).await,
        [
            ("EW70+".to_string(), Some(70.0), None),
            ("EWU11".to_string(), None, Some(11.0)),
            ("EWU13".to_string(), Some(11.0), Some(13.0)),
            ("U9,5".to_string(), None, Some(9.5)),
        ]
    );
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('categories')")
            .fetch_all(&db)
            .await
            .unwrap();
    assert!(
        !columns.iter().any(|c| c.contains("birthday")),
        "{columns:?}"
    );
}

#[tokio::test]
async fn the_migration_stops_at_a_birthday_bound_off_the_determining_day() {
    let db = migrated_before_ages().await;
    insert_dated(&db, "EWU11", Some("2016-06-01T00:00:00Z"), None).await;
    insert_dated(
        &db,
        "Krumm",
        Some("2014-05-15T00:00:00Z"),
        Some("2016-06-01T00:00:00Z"),
    )
    .await;

    // One connection for the failing migration and the check after it: sqlx
    // rolls the failed migration back only when its connection is used next,
    // and until then its schema lock fails every other connection with
    // "database schema is locked" (the in-memory database is a shared cache).
    let mut conn = db.acquire().await.unwrap();
    let error = sqlx::migrate!("./migrations")
        .run(&mut *conn)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("Krumm"), "{error}");
    assert!(error.contains("2014-05-15"), "{error}");
    // Nothing changed.
    let from: String =
        sqlx::query_scalar("SELECT from_birthday FROM categories WHERE name = 'Krumm'")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(from, "2014-05-15T00:00:00Z");
}

#[tokio::test]
async fn reverting_the_migration_writes_the_derived_birthday_bounds() {
    let db = migrated_before_ages().await;
    insert_dated(
        &db,
        "EWU13",
        Some("2014-06-01T00:00:00Z"),
        Some("2016-06-01T00:00:00Z"),
    )
    .await;
    insert_dated(&db, "EW70+", None, Some("1957-06-01T00:00:00Z")).await;
    let migrator = sqlx::migrate!("./migrations");
    migrator.run(&db).await.unwrap();

    migrator.undo(&db, BEFORE_AGES).await.unwrap();

    let bounds: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT name, date(from_birthday), date(to_birthday) FROM categories ORDER BY name",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(
        bounds,
        [
            ("EW70+".into(), "1900-01-01".into(), "1957-06-01".into()),
            ("EWU13".into(), "2014-06-01".into(), "2016-06-01".into()),
        ]
    );
}

/// Adds a female rider born on `birthdate` with a single and returns the
/// single's category.
async fn single_category(
    app: &TestApp,
    cookie: &str,
    club_id: ClubId,
    name: &str,
    birthdate: &str,
) -> Value {
    let mut body = rider_body(club_id, name, true, false);
    body["birthdate"] = json!(format!("{birthdate}T00:00:00.000Z"));
    let rider = app.add_rider(cookie, body).await;
    let act = app.act_of(rider).await;
    let response = app
        .get(&format!("/api/query/get_act?act_id={act}"), None)
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body["category"].clone()
}

async fn set_ages(app: &TestApp, name: &str, min_age: Option<f64>, under_age: Option<f64>) {
    sqlx::query("UPDATE categories SET min_age = $1, under_age = $2 WHERE name = $3")
        .bind(min_age)
        .bind(under_age)
        .bind(name)
        .execute(&app.db)
        .await
        .unwrap();
}

async fn set_determining_day(app: &TestApp, day: &str) {
    sqlx::query("UPDATE competition SET determining_day = $1")
        .bind(day)
        .execute(&app.db)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_birthday_on_a_derived_bound_belongs_to_the_younger_category() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    // Determining day 2027-06-01: U11 from 2016-06-01, U13 from 2014-06-01.
    assert_eq!(
        single_category(&app, &cookie, club_id, "Anna", "2016-06-01").await,
        "EWU11"
    );
    assert_eq!(
        single_category(&app, &cookie, club_id, "Berta", "2016-05-31").await,
        "EWU13"
    );
    assert_eq!(
        single_category(&app, &cookie, club_id, "Carla", "2014-06-01").await,
        "EWU13"
    );
}

#[tokio::test]
async fn fractional_ages_count_in_months() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    // U11 becomes U9,5 (9 years 6 months): from 2017-12-01.
    set_ages(&app, "EWU11", None, Some(9.5)).await;
    set_ages(&app, "EWU13", Some(9.5), Some(13.0)).await;
    assert_eq!(
        single_category(&app, &cookie, club_id, "Anna", "2017-12-01").await,
        "EWU11"
    );
    assert_eq!(
        single_category(&app, &cookie, club_id, "Berta", "2017-11-30").await,
        "EWU13"
    );
}

#[tokio::test]
async fn a_determining_day_on_29_february_counts_from_the_28th() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    set_determining_day(&app, "2028-02-29").await;
    // 11 years before 2028-02-29 is 2017-02-28, as `years_before` has it.
    assert_eq!(
        single_category(&app, &cookie, club_id, "Anna", "2017-02-28").await,
        "EWU11"
    );
    assert_eq!(
        single_category(&app, &cookie, club_id, "Berta", "2017-02-27").await,
        "EWU13"
    );
}

#[tokio::test]
async fn a_rider_born_on_29_february_is_placed_by_the_day() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    // Determining day 2027-06-01: U13 from 2014-06-01 to before 2016-06-01.
    assert_eq!(
        single_category(&app, &cookie, club_id, "Anna", "2016-02-29").await,
        "EWU13"
    );
}

#[tokio::test]
async fn open_ends_take_the_youngest_and_the_oldest() {
    let app = TestApp::new(common::in_memory_migrated().await).await;
    let (cookie, club_id) = app.create_club_owner("Owner", "RSV").await;
    assert_eq!(
        single_category(&app, &cookie, club_id, "Anna", "2026-01-01").await,
        "EWU11"
    );
    assert_eq!(
        single_category(&app, &cookie, club_id, "Berta", "1930-01-01").await,
        "EW70+"
    );
}
