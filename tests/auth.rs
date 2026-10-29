//! Integration tests for registration, login and account recovery.

mod common;

use axum::http::StatusCode;
use common::{BASE_URL, PASSWORD, TestApp, closed_registration, email_for, mailpit::FROM_ADDRESS};
use serde_json::json;
use uuid::Uuid;

fn register_body(name: &str, password: &str) -> serde_json::Value {
    json!({ "name": name, "email": email_for(name), "password": password })
}

#[tokio::test]
async fn register_creates_unverified_user_logs_in_and_sends_mail() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_mailpit(db).await;

    let response = app
        .post(
            "/api/command/register",
            register_body("Alex", PASSWORD),
            None,
        )
        .await;

    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let cookie = response.jwt_cookie().expect("register sets jwt cookie");
    let user_id = response.body["user_id"].as_str().unwrap();

    let whoami = app.get("/api/query/whoami", Some(&cookie)).await;
    assert_eq!(whoami.status, StatusCode::OK);
    assert_eq!(whoami.body["id"], user_id);
    assert_eq!(whoami.body["email"], email_for("Alex"));
    assert_eq!(whoami.body["email_verified"], false);
    assert_eq!(whoami.body["roles"], serde_json::json!([]));

    let mail = app.single_mail().await;
    assert_eq!(mail.from, FROM_ADDRESS);
    assert_eq!(mail.to, vec![email_for("Alex")]);
    assert_eq!(mail.subject, "Einradevent 2027 (Demo) - Email bestätigen");
    assert!(mail.text.starts_with("Hallo Alex,"), "{}", mail.text);
    assert!(
        mail.text.contains("Dein Einradevent 2027 (Demo) Team")
            && !mail.text.contains("Freestyle Cup"),
        "{}",
        mail.text
    );
    assert!(
        mail.link()
            .starts_with(&format!("{BASE_URL}/verify_email?token=")),
        "{}",
        mail.link()
    );
}

#[tokio::test]
async fn register_reports_mail_delivery_failure() {
    let db = common::in_memory_migrated().await;
    // No mail server is reachable for this app.
    let app = TestApp::new(db).await;

    let response = app
        .post(
            "/api/command/register",
            register_body("Alex", PASSWORD),
            None,
        )
        .await;

    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        response.body["message"]
            .as_str()
            .unwrap()
            .starts_with("Mail error"),
        "{:?}",
        response.body
    );
    assert!(response.jwt_cookie().is_none());
}

#[tokio::test]
async fn register_stores_hashed_password() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_mailpit(db).await;
    app.post(
        "/api/command/register",
        register_body("Alex", PASSWORD),
        None,
    )
    .await;

    let stored: String = sqlx::query_scalar("SELECT password FROM users WHERE name = 'Alex'")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_ne!(stored, PASSWORD);
    assert!(password_auth::verify_password(PASSWORD, &stored).is_ok());
}

#[tokio::test]
async fn register_rejects_weak_password() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_mailpit(db).await;

    let response = app
        .post(
            "/api/command/register",
            register_body("Alex", "password"),
            None,
        )
        .await;

    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.body["type"], "Generic");
    assert!(
        response.body["message"]
            .as_str()
            .unwrap()
            .contains("Kategorien")
    );
    assert!(app.mails().await.is_empty());
}

#[tokio::test]
async fn register_rejects_duplicate_email() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_mailpit(db).await;
    app.post(
        "/api/command/register",
        register_body("Alex", PASSWORD),
        None,
    )
    .await;

    let mut body = register_body("Other Name", PASSWORD);
    body["email"] = json!(email_for("Alex"));
    let response = app.post("/api/command/register", body, None).await;

    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        response.body["message"]
            .as_str()
            .unwrap()
            .contains("bereits registriert")
    );
    assert_eq!(app.mails().await.len(), 1);
}

#[tokio::test]
async fn register_is_forbidden_outside_registration_period() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_status(db, closed_registration()).await;

    let response = app
        .post(
            "/api/command/register",
            register_body("Alex", PASSWORD),
            None,
        )
        .await;

    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert_eq!(
        response.body,
        json!({ "type": "StatusCode", "message": 403 })
    );
}

#[tokio::test]
async fn verify_email_marks_user_verified_and_consumes_token() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_mailpit(db).await;
    app.post(
        "/api/command/register",
        register_body("Alex", PASSWORD),
        None,
    )
    .await;
    let token = app.single_mail().await.token();

    let response = app
        .post("/api/command/verify_email", json!({ "token": token }), None)
        .await;

    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let cookie = response.jwt_cookie().expect("verify_email sets jwt cookie");
    let whoami = app.get("/api/query/whoami", Some(&cookie)).await;
    assert_eq!(whoami.body["email_verified"], true);

    let reused = app
        .post("/api/command/verify_email", json!({ "token": token }), None)
        .await;
    assert_eq!(reused.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn resend_mail_validation_replaces_the_previous_token() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_mailpit(db).await;
    let register = app
        .post(
            "/api/command/register",
            register_body("Alex", PASSWORD),
            None,
        )
        .await;
    let cookie = register.jwt_cookie().unwrap();

    let response = app
        .post(
            "/api/command/resend_mail_validation",
            json!({}),
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);

    let mails = app.mails().await;
    assert_eq!(mails.len(), 2);
    assert_eq!(mails[1].to, vec![email_for("Alex")]);
    assert_eq!(
        mails[1].subject,
        "Einradevent 2027 (Demo) - Email bestätigen"
    );
    let (old_token, new_token) = (mails[0].token(), mails[1].token());
    assert_ne!(old_token, new_token);

    let old = app
        .post(
            "/api/command/verify_email",
            json!({ "token": old_token }),
            None,
        )
        .await;
    assert_eq!(old.status, StatusCode::NOT_FOUND);
    let new = app
        .post(
            "/api/command/verify_email",
            json!({ "token": new_token }),
            None,
        )
        .await;
    assert_eq!(new.status, StatusCode::OK);
}

#[tokio::test]
async fn resend_mail_validation_requires_login() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_mailpit(db).await;
    let response = app
        .post("/api/command/resend_mail_validation", json!({}), None)
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert!(app.mails().await.is_empty());
}

#[tokio::test]
async fn verify_email_with_unknown_token_is_not_found() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let response = app
        .post(
            "/api/command/verify_email",
            json!({ "token": Uuid::new_v4() }),
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn login_with_correct_password_sets_cookie() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let user_id = app.create_user("Alex", false).await;

    let response = app
        .post(
            "/api/command/login",
            json!({ "email": email_for("Alex"), "password": PASSWORD }),
            None,
        )
        .await;

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body["user_id"], user_id.to_string());
    let cookie = response.jwt_cookie().unwrap();
    let whoami = app.get("/api/query/whoami", Some(&cookie)).await;
    assert_eq!(whoami.body["id"], user_id.to_string());
}

#[tokio::test]
async fn login_with_wrong_password_or_unknown_email_is_unauthorized() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    app.create_user("Alex", false).await;

    for body in [
        json!({ "email": email_for("Alex"), "password": "Falsch-Passwort1" }),
        json!({ "email": "nobody@cup.test", "password": PASSWORD }),
    ] {
        let response = app.post("/api/command/login", body, None).await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED);
        assert_eq!(response.body, json!({ "type": "InvalidCredentials" }));
        assert!(response.jwt_cookie().is_none());
    }
}

#[tokio::test]
async fn logout_clears_jwt_cookie() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;

    let response = app.post("/api/command/logout", json!(null), None).await;

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.jwt_cookie().as_deref(), Some("jwt="));
}

#[tokio::test]
async fn whoami_requires_valid_jwt() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;

    let missing = app.get("/api/query/whoami", None).await;
    assert_eq!(missing.status, StatusCode::UNAUTHORIZED);

    let invalid = app.get("/api/query/whoami", Some("jwt=not-a-token")).await;
    assert_eq!(invalid.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn jwt_for_deleted_user_is_rejected() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let cookie = app.cookie_for(Uuid::now_v7());

    let response = app.get("/api/query/whoami", Some(&cookie)).await;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(response.body, "User not found");
}

#[tokio::test]
async fn password_reset_flow_replaces_password() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_mailpit(db).await;
    app.create_user("Alex", false).await;

    let request = app
        .post(
            "/api/command/request_password_reset",
            json!({ "email": email_for("Alex") }),
            None,
        )
        .await;
    assert_eq!(request.status, StatusCode::OK, "{:?}", request.body);
    let mail = app.single_mail().await;
    assert_eq!(mail.to, vec![email_for("Alex")]);
    assert_eq!(
        mail.subject,
        "Einradevent 2027 (Demo) - Passwort zurücksetzen"
    );
    assert!(mail.text.starts_with("Hallo Alex,"), "{}", mail.text);
    assert!(
        mail.text.contains("Dein Einradevent 2027 (Demo) Team")
            && !mail.text.contains("Freestyle Cup"),
        "{}",
        mail.text
    );
    assert!(
        mail.link()
            .starts_with(&format!("{BASE_URL}/reset_password?token=")),
        "{}",
        mail.link()
    );

    let token = mail.token();
    let new_password = "Neues-Passwort2";
    let reset = app
        .post(
            "/api/command/reset_password",
            json!({ "token": token, "new_password": new_password }),
            None,
        )
        .await;
    assert_eq!(reset.status, StatusCode::OK, "{:?}", reset.body);

    let old_login = app
        .post(
            "/api/command/login",
            json!({ "email": email_for("Alex"), "password": PASSWORD }),
            None,
        )
        .await;
    assert_eq!(old_login.status, StatusCode::UNAUTHORIZED);

    let new_login = app
        .post(
            "/api/command/login",
            json!({ "email": email_for("Alex"), "password": new_password }),
            None,
        )
        .await;
    assert_eq!(new_login.status, StatusCode::OK);

    let reused = app
        .post(
            "/api/command/reset_password",
            json!({ "token": token, "new_password": "Noch-Eins3" }),
            None,
        )
        .await;
    assert_eq!(reused.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn requesting_a_new_reset_invalidates_the_previous_token() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_mailpit(db).await;
    app.create_user("Alex", false).await;
    let body = json!({ "email": email_for("Alex") });

    app.post("/api/command/request_password_reset", body.clone(), None)
        .await;
    app.post("/api/command/request_password_reset", body, None)
        .await;
    let mails = app.mails().await;
    assert_eq!(mails.len(), 2);
    let (first, second) = (mails[0].token(), mails[1].token());

    assert_ne!(first, second);
    let response = app
        .post(
            "/api/command/reset_password",
            json!({ "token": first, "new_password": "Neues-Passwort2" }),
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reset_password_rejects_weak_password_and_keeps_token() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_mailpit(db).await;
    app.create_user("Alex", false).await;
    app.post(
        "/api/command/request_password_reset",
        json!({ "email": email_for("Alex") }),
        None,
    )
    .await;
    let token = app.single_mail().await.token();

    let response = app
        .post(
            "/api/command/reset_password",
            json!({ "token": token, "new_password": "kurz" }),
            None,
        )
        .await;

    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);

    let retry = app
        .post(
            "/api/command/reset_password",
            json!({ "token": token, "new_password": "Neues-Passwort2" }),
            None,
        )
        .await;
    assert_eq!(retry.status, StatusCode::OK, "{:?}", retry.body);
}

#[tokio::test]
async fn password_reset_for_unknown_email_is_not_found() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_mailpit(db).await;

    let response = app
        .post(
            "/api/command/request_password_reset",
            json!({ "email": "nobody@cup.test" }),
            None,
        )
        .await;

    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert!(app.mails().await.is_empty());
}
