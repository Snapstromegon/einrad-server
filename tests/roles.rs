//! Integration tests for roles: the migration from the admin flag, what
//! `whoami` reports and how a missing permission is rejected.

mod common;

use axum::http::StatusCode;
use common::{TestApp, email_for, rider_body};
use einrad_server::ids::UserId;
use serde_json::{Value, json};

/// The migration that replaced `users.is_admin` with roles.
const ROLES_MIGRATION: i64 = 20261023190000;

fn strings(value: &Value) -> Vec<&str> {
    let mut strings: Vec<_> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    strings.sort_unstable();
    strings
}

#[tokio::test]
async fn admins_from_before_roles_become_admins() {
    let db = common::in_memory().await;
    let migrator = sqlx::migrate!();
    migrator.run_to(ROLES_MIGRATION - 1, &db).await.unwrap();
    let admin = UserId::new();
    let user = UserId::new();
    for (id, name, is_admin) in [(admin, "Admin", true), (user, "Alex", false)] {
        sqlx::query(
            "INSERT INTO users (id, email, email_verified, name, password, is_admin) VALUES ($1, $2, true, $3, 'x', $4)",
        )
        .bind(id)
        .bind(email_for(name))
        .bind(name)
        .bind(is_admin)
        .execute(&db)
        .await
        .unwrap();
    }
    migrator.run(&db).await.unwrap();
    let app = TestApp::new(db).await;

    let admin = app
        .get("/api/query/whoami", Some(&app.cookie_for(admin)))
        .await;
    assert_eq!(admin.status, StatusCode::OK, "{:?}", admin.body);
    assert_eq!(strings(&admin.body["roles"]), ["admin"]);

    let user = app
        .get("/api/query/whoami", Some(&app.cookie_for(user)))
        .await;
    assert_eq!(strings(&user.body["roles"]), Vec::<&str>::new());
}

#[tokio::test]
async fn whoami_lists_roles_and_their_permissions() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let id = app
        .create_user_with_roles("Kim", &["treasurer", "music_check"])
        .await;

    let response = app
        .get("/api/query/whoami", Some(&app.cookie_for(id)))
        .await;

    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_eq!(
        strings(&response.body["roles"]),
        ["music_check", "treasurer"]
    );
    assert_eq!(
        strings(&response.body["permissions"]),
        [
            "check_songs",
            "mail_clubs",
            "manage_payments",
            "view_club_balances",
            "view_club_contacts",
            "view_clubs",
            "view_riders"
        ]
    );
}

#[tokio::test]
async fn admins_hold_every_permission() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let kim = app.create_user_with_roles("Kim", &["jury_lead"]).await;

    let response = app.get("/api/query/whoami", Some(&admin)).await;
    let jury_lead = app
        .get("/api/query/whoami", Some(&app.cookie_for(kim)))
        .await;

    assert_eq!(strings(&response.body["roles"]), ["admin"]);
    let permissions = strings(&response.body["permissions"]);
    assert!(permissions.contains(&"manage_users"), "{permissions:?}");
    assert!(permissions.contains(&"manage_backups"), "{permissions:?}");
    for permission in strings(&jury_lead.body["permissions"]) {
        assert!(permissions.contains(&permission), "{permission}");
    }
}

#[tokio::test]
async fn a_missing_permission_is_forbidden_a_missing_login_unauthorized() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let treasurer = app.create_user_with_roles("Kim", &["treasurer"]).await;

    let forbidden = app
        .get("/api/query/list_users", Some(&app.cookie_for(treasurer)))
        .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);

    let anonymous = app.get("/api/query/list_users", None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);

    let admin = app.create_admin().await;
    let allowed = app.get("/api/query/list_users", Some(&admin)).await;
    assert_eq!(allowed.status, StatusCode::OK);
}

#[tokio::test]
async fn admins_grant_and_revoke_several_roles_at_once() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let kim = app.create_user_with_roles("Kim", &["treasurer"]).await;
    let set = |roles: Value| json!({ "user_id": kim, "roles": roles });

    let response = app
        .post(
            "/api/command/set_user_roles",
            set(json!(["check_in", "music_control"])),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let whoami = app
        .get("/api/query/whoami", Some(&app.cookie_for(kim)))
        .await;
    assert_eq!(
        strings(&whoami.body["roles"]),
        ["check_in", "music_control"]
    );

    let users = app.get("/api/query/list_users", Some(&admin)).await;
    let listed = users
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|user| user["id"] == kim.to_string())
        .unwrap();
    assert_eq!(strings(&listed["roles"]), ["check_in", "music_control"]);

    let unknown = app
        .post(
            "/api/command/set_user_roles",
            set(json!(["dj"])),
            Some(&admin),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn admins_keep_their_admin_role_but_may_change_the_others() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let admin_id = app.create_user("Admin", true).await;
    let admin = app.cookie_for(admin_id);
    let set = |roles: Value| json!({ "user_id": admin_id, "roles": roles });

    let response = app
        .post(
            "/api/command/set_user_roles",
            set(json!(["admin", "music_control"])),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let response = app
        .post(
            "/api/command/set_user_roles",
            set(json!(["music_control"])),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);

    let whoami = app.get("/api/query/whoami", Some(&admin)).await;
    assert_eq!(strings(&whoami.body["roles"]), ["admin", "music_control"]);
}

#[tokio::test]
async fn only_admins_set_roles() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let jury_lead = app.create_user_with_roles("Jury", &["jury_lead"]).await;
    let body = json!({ "user_id": jury_lead, "roles": ["jury_lead", "treasurer"] });

    let response = app
        .post(
            "/api/command/set_user_roles",
            body,
            Some(&app.cookie_for(jury_lead)),
        )
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
}

/// A user holding `roles`, with their cookie.
async fn member(app: &TestApp, name: &str, roles: &[&str]) -> String {
    app.cookie_for(app.create_user_with_roles(name, roles).await)
}

async fn status(app: &TestApp, uri: &str, body: Value, cookie: &str) -> StatusCode {
    app.post(uri, body, Some(cookie)).await.status
}

#[tokio::test]
async fn each_role_reaches_its_own_commands_only() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Clara", "SSV").await;
    let rider = app
        .add_rider(&owner, rider_body(club_id, "Anna", true, false))
        .await;
    let act_id = app.act_of(rider).await;
    let payment = || json!({ "club_id": club_id, "amount": 20.0, "date": "2027-03-01" });
    let check_in = || json!({ "rider_ids": [rider], "checked_in": true });
    let song = || json!({ "act_id": act_id, "checked": true });
    let order = || json!({ "act_id": act_id, "order": 1 });

    let treasurer = member(&app, "Kasse", &["treasurer"]).await;
    assert_eq!(
        status(&app, "/api/command/create_booking", payment(), &treasurer).await,
        StatusCode::OK
    );
    assert_eq!(
        status(&app, "/api/command/set_checked_in", check_in(), &treasurer).await,
        StatusCode::FORBIDDEN
    );

    let desk = member(&app, "Desk", &["check_in"]).await;
    assert_eq!(
        status(&app, "/api/command/set_checked_in", check_in(), &desk).await,
        StatusCode::OK
    );
    assert_eq!(
        status(&app, "/api/command/create_booking", payment(), &desk).await,
        StatusCode::FORBIDDEN
    );

    let music = member(&app, "Musik", &["music_control"]).await;
    assert_eq!(
        status(&app, "/api/command/timeplan_forward", json!(null), &music).await,
        StatusCode::OK
    );
    assert_eq!(
        status(&app, "/api/command/set_checked_in", check_in(), &music).await,
        StatusCode::OK
    );
    assert_eq!(
        status(&app, "/api/command/create_booking", payment(), &music).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(&app, "/api/command/set_act_order", order(), &music).await,
        StatusCode::FORBIDDEN
    );

    let checker = member(&app, "Prüfer", &["music_check"]).await;
    assert_eq!(
        status(&app, "/api/command/set_song_checked", song(), &checker).await,
        StatusCode::OK
    );
    assert_eq!(
        status(&app, "/api/command/set_act_order", order(), &checker).await,
        StatusCode::FORBIDDEN
    );

    let moderation = member(&app, "Moderation", &["moderation"]).await;
    assert_eq!(
        status(
            &app,
            "/api/command/timeplan_forward",
            json!(null),
            &moderation
        )
        .await,
        StatusCode::FORBIDDEN
    );

    let jury_lead = member(&app, "Jury", &["jury_lead"]).await;
    assert_eq!(
        app.get("/api/query/list_judges", Some(&jury_lead))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        status(&app, "/api/command/create_booking", payment(), &jury_lead).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.get("/api/query/list_riders", Some(&jury_lead))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.get("/api/query/list_judges", Some(&treasurer))
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    let admin = app.create_admin().await;
    assert_eq!(
        status(&app, "/api/command/set_act_order", order(), &admin).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_check_in_but_not_the_treasurer_sees_birthdates() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Clara", "SSV").await;
    app.add_rider(&owner, rider_body(club_id, "Anna", true, false))
        .await;

    let treasurer = member(&app, "Kasse", &["treasurer"]).await;
    let riders = app.get("/api/query/list_riders", Some(&treasurer)).await;
    assert_eq!(riders.status, StatusCode::OK, "{:?}", riders.body);
    assert_eq!(riders.body[0]["firstname"], "Anna");
    assert_eq!(riders.body[0]["birthdate"], Value::Null);
    assert_eq!(riders.body[0]["age_on_competition"], Value::Null);
    let acts = app.get("/api/query/list_acts", Some(&treasurer)).await;
    assert_eq!(acts.body[0]["max_age"], Value::Null);

    // For the ID and age check at the desk.
    let desk = member(&app, "Desk", &["check_in"]).await;
    let riders = app.get("/api/query/list_riders", Some(&desk)).await;
    assert_ne!(riders.body[0]["birthdate"], Value::Null);
    assert_ne!(riders.body[0]["age_on_competition"], Value::Null);
    let acts = app.get("/api/query/list_acts", Some(&desk)).await;
    assert_ne!(acts.body[0]["max_age"], Value::Null);
}

#[tokio::test]
async fn check_in_sees_club_balances_but_not_contacts() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, club_id) = app.create_club_owner("Clara", "SSV").await;
    app.add_rider(&owner, rider_body(club_id, "Anna", true, false))
        .await;
    let admin = app.create_admin().await;
    app.post(
        "/api/command/create_booking",
        json!({ "club_id": club_id, "amount": 5.0, "date": "2027-03-01" }),
        Some(&admin),
    )
    .await;

    let desk = member(&app, "Desk", &["check_in"]).await;
    let clubs = app.get("/api/query/list_clubs", Some(&desk)).await;
    assert_eq!(clubs.status, StatusCode::OK, "{:?}", clubs.body);
    let club = &clubs.body[0];
    assert_eq!(club["name"], "SSV");
    assert_eq!(club["paid"], 5.0);
    assert!(club["amount_due"]["total"].as_f64().unwrap() > 0.0);
    assert_eq!(club["contacts"], Value::Null);

    let jury_lead = member(&app, "Jury", &["jury_lead"]).await;
    let clubs = app.get("/api/query/list_clubs", Some(&jury_lead)).await;
    let club = &clubs.body[0];
    assert_eq!(club["contacts"]["contact_email"], email_for("Clara"));
    assert_eq!(club["paid"], Value::Null);
    assert_eq!(club["amount_due"], Value::Null);

    let musik = member(&app, "Musik", &["music_control"]).await;
    let clubs = app.get("/api/query/list_clubs", Some(&musik)).await;
    assert_eq!(clubs.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_role_doesnt_open_other_clubs() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (_, club_id) = app.create_club_owner("Clara", "SSV").await;
    let treasurer = member(&app, "Kasse", &["treasurer", "jury_lead", "check_in"]).await;

    // Renaming someone else's club changes nothing.
    let rename = json!({ "club_id": club_id, "name": "Hijacked" });
    app.post("/api/command/rename_club", rename.clone(), Some(&treasurer))
        .await;
    let clubs = app.get("/api/query/list_clubs", Some(&treasurer)).await;
    assert_eq!(clubs.body[0]["name"], "SSV");
    let add = rider_body(club_id, "Eva", true, false);
    assert_eq!(
        status(&app, "/api/command/add_rider", add, &treasurer).await,
        StatusCode::FORBIDDEN
    );
    let admin = app.create_admin().await;
    assert_eq!(
        status(&app, "/api/command/rename_club", rename, &admin).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn only_registration_managers_edit_after_the_meldeschluss() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (_, club_id) = app.create_club_owner("Clara", "SSV").await;
    app.close_registration().await;
    let status_of = |body: &Value| body["can_register_rider"].clone();

    let jury_lead = member(&app, "Jury", &["jury_lead"]).await;
    let response = app
        .get("/api/query/get_system_status", Some(&jury_lead))
        .await;
    assert_eq!(status_of(&response.body), false, "{:?}", response.body);

    let admin = app.create_admin().await;
    let response = app.get("/api/query/get_system_status", Some(&admin)).await;
    assert_eq!(status_of(&response.body), true);
    let add = rider_body(club_id, "Eva", true, false);
    assert_eq!(
        status(&app, "/api/command/add_rider", add, &admin).await,
        StatusCode::OK
    );
}
