//! Integration tests for club mails (Rundmail, #257).

mod common;

use axum::http::StatusCode;
use common::{TestApp, TestResponse};
use serde_json::{Value, json};
use uuid::Uuid;

/// A club whose contact is `contact` and, with `representative`, whose club
/// representative has that name and email.
async fn club(
    app: &TestApp,
    contact: &str,
    club_name: &str,
    representative: Option<(&str, &str)>,
) -> (String, Uuid) {
    let (cookie, club_id) = app.create_club_owner(contact, club_name).await;
    if let Some((name, email)) = representative {
        let response = app
            .post(
                "/api/command/set_club_responsible",
                json!({ "club_id": club_id, "name": name, "email": email }),
                Some(&cookie),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    }
    (cookie, club_id)
}

/// The competition's contact email, where answers to club mails go.
const CONTACT_EMAIL: &str = "orga@cup.test";

/// An app sending to Mailpit whose competition has a contact email, so club
/// mails go out exactly as written.
async fn mail_app() -> TestApp {
    let app = TestApp::with_mailpit(common::in_memory_migrated().await).await;
    set_contact_email(&app, Some(CONTACT_EMAIL)).await;
    app
}

async fn set_contact_email(app: &TestApp, email: Option<&str>) {
    sqlx::query("UPDATE competition SET contact_email = ?")
        .bind(email)
        .execute(&app.db)
        .await
        .unwrap();
}

async fn send(app: &TestApp, cookie: &str, mail: Value) -> TestResponse {
    app.post("/api/command/send_club_mail", mail, Some(cookie))
        .await
}

fn sorted(mut addresses: Vec<String>) -> Vec<String> {
    addresses.sort();
    addresses
}

#[tokio::test]
async fn a_club_mail_goes_to_the_club_contact_and_the_representative() {
    let app = mail_app().await;
    let admin = app.create_admin().await;
    let (_, rsv) = club(
        &app,
        "Max Mustermann",
        "RSV Muster",
        Some(("Erika Muster", "erika@cup.test")),
    )
    .await;
    club(&app, "Lea Beispiel", "TV Beispiel", None).await;

    let response = send(
        &app,
        &admin,
        json!({
            "subject": "Neuigkeiten für {{ club_name }}",
            "body": "Hallo {{ contact_name }},\nneue Infos für {{ club_name }}.",
            "club_ids": [rsv],
        }),
    )
    .await;

    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let mail = app.single_mail().await;
    assert_eq!(
        sorted(mail.to.clone()),
        ["erika@cup.test", "max.mustermann@cup.test"]
    );
    assert_eq!(mail.subject, "Neuigkeiten für RSV Muster");
    assert_eq!(
        mail.text.trim_end(),
        "Hallo Max Mustermann,\nneue Infos für RSV Muster."
    );
}

/// Asserts the mail was refused with a message containing `message`.
fn assert_refused(response: &TestResponse, message: &str) {
    assert_eq!(
        response.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{:?}",
        response.body
    );
    let text = response.body["message"].as_str().unwrap_or_default();
    assert!(text.contains(message), "{text:?} lacks {message:?}");
}

#[tokio::test]
async fn a_template_with_an_unknown_variable_or_an_include_sends_nothing() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_mailpit(db).await;
    let admin = app.create_admin().await;
    let (_, rsv) = club(&app, "Max Mustermann", "RSV Muster", None).await;

    let typo = send(
        &app,
        &admin,
        json!({ "subject": "Info", "body": "Hallo {{ club_nmae }}", "club_ids": [rsv] }),
    )
    .await;
    let include = send(
        &app,
        &admin,
        json!({ "subject": "Info", "body": "{% include \"/etc/passwd\" %}", "club_ids": [rsv] }),
    )
    .await;
    let broken = send(
        &app,
        &admin,
        json!({ "subject": "Info {{ club_name", "body": "Text", "club_ids": [rsv] }),
    )
    .await;

    assert_refused(&typo, "club_nmae");
    assert_refused(&include, "Fehler im Text");
    assert_refused(&broken, "Fehler im Betreff");
    assert!(app.mails().await.is_empty());
}

#[tokio::test]
async fn answers_go_to_the_contact_email_and_without_one_the_mail_says_not_to_answer() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::with_mailpit(db).await;
    let admin = app.create_admin().await;
    let (_, rsv) = club(&app, "Max Mustermann", "RSV Muster", None).await;
    let mail = json!({ "subject": "Info", "body": "Text\n", "club_ids": [rsv] });

    send(&app, &admin, mail.clone()).await;
    set_contact_email(&app, Some(CONTACT_EMAIL)).await;
    send(&app, &admin, mail).await;

    let mails = app.mails().await;
    assert_eq!(mails.len(), 2);
    assert!(mails[0].reply_to.is_empty(), "{:?}", mails[0].reply_to);
    assert_eq!(
        mails[0].text.trim_end(),
        "Text\n\nP.S.: Bitte nicht auf diese Mail antworten - das Postfach wird nicht gelesen."
    );
    assert_eq!(mails[1].reply_to, [CONTACT_EMAIL]);
    assert_eq!(mails[1].text.trim_end(), "Text");
}

/// The club code of `club_id`, as the club list shows it.
async fn club_code(app: &TestApp, club_id: Uuid) -> String {
    let admin = app.create_admin().await;
    let clubs = app.get("/api/query/list_clubs", Some(&admin)).await;
    clubs
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == json!(club_id))
        .unwrap()["code"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn the_treasurer_mails_amounts_and_the_jury_lead_missing_deployments() {
    let app = mail_app().await;
    let treasurer = app.cookie_for(
        app.create_user_with_roles("Kim Kasse", &["treasurer"])
            .await,
    );
    let jury_lead = app.cookie_for(app.create_user_with_roles("Jo Jury", &["jury_lead"]).await);
    let (owner, rsv) = club(&app, "Max Mustermann", "RSV Muster", None).await;
    // One single: a 15 € start, and 4 missing jury deployments at 7 €.
    app.add_single_act(&owner, rsv, "Lea").await;
    let payment = app
        .post(
            "/api/command/create_booking",
            json!({ "club_id": rsv, "amount": 20, "date": "2027-03-01" }),
            Some(&treasurer),
        )
        .await;
    assert_eq!(payment.status, StatusCode::OK, "{:?}", payment.body);
    let code = club_code(&app, rsv).await;

    let money = send(
        &app,
        &treasurer,
        json!({
            "subject": "Startgeld",
            "body": "{{ club_code }}: {{ amount_due | euro }} fällig, {{ paid | euro }} gezahlt, {{ open_amount | euro }} offen, {{ missing_deployments }} Einsätze fehlen.",
            "club_ids": [rsv],
        }),
    )
    .await;
    let jury_money = send(
        &app,
        &jury_lead,
        json!({ "subject": "Startgeld", "body": "{{ open_amount }}", "club_ids": [rsv] }),
    )
    .await;
    let jury = send(
        &app,
        &jury_lead,
        json!({ "subject": "Jury", "body": "{{ missing_deployments }} fehlen", "club_ids": [rsv] }),
    )
    .await;

    assert_eq!(money.status, StatusCode::OK, "{:?}", money.body);
    assert_refused(&jury_money, "open_amount");
    assert_eq!(jury.status, StatusCode::OK, "{:?}", jury.body);
    let mails = app.mails().await;
    assert_eq!(mails.len(), 2);
    assert_eq!(
        mails[0].text.trim_end(),
        format!("{code}: 43,00 € fällig, 20,00 € gezahlt, 23,00 € offen, 4 Einsätze fehlen.")
    );
    assert_eq!(mails[1].text.trim_end(), "4 fehlen");
}

#[tokio::test]
async fn only_admins_the_jury_lead_and_the_treasurer_send_club_mails() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let (owner, rsv) = club(&app, "Max Mustermann", "RSV Muster", None).await;
    let check_in = app.cookie_for(app.create_user_with_roles("Cem Check", &["check_in"]).await);
    let mail = json!({ "subject": "Info", "body": "Text", "club_ids": [rsv] });

    for cookie in [&owner, &check_in] {
        let response = send(&app, cookie, mail.clone()).await;
        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "{:?}",
            response.body
        );
    }
}

#[tokio::test]
async fn missing_music_lists_the_clubs_acts_without_music() {
    let app = mail_app().await;
    let admin = app.create_admin().await;
    let (owner, rsv) = club(&app, "Max Mustermann", "RSV Muster", None).await;
    let (_, tv) = club(&app, "Lea Beispiel", "TV Beispiel", None).await;
    app.add_single_act(&owner, rsv, "Lea").await;
    let with_music = app.add_single_act(&owner, rsv, "Mia").await;
    let upload = app
        .post_file(
            &format!("/api/command/save_act_song?act_id={with_music}"),
            "Lied.mp3",
            b"ID3 not really an mp3",
            Some(&owner),
        )
        .await;
    assert_eq!(upload.status, StatusCode::OK, "{:?}", upload.body);
    app.create_group(&owner, rsv, "Wirbelwind").await;

    let response = send(
        &app,
        &admin,
        json!({
            "subject": "Musik",
            "body": "{% for act in missing_music %}- {{ act.type }}: {{ act.name }}\n{% endfor %}",
            "club_ids": [rsv, tv],
        }),
    )
    .await;
    let typo = send(
        &app,
        &admin,
        json!({
            "subject": "Musik",
            "body": "{% for act in missing_music %}{{ act.nmae }}{% endfor %}",
            "club_ids": [tv],
        }),
    )
    .await;

    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    assert_refused(&typo, "Fehler im Text");
    let mails = app.mails().await;
    assert_eq!(mails.len(), 2);
    assert_eq!(
        mails[0].text.trim_end(),
        "- Einzelkür: Lea Muster\n- Kleingruppe: Wirbelwind"
    );
    assert_eq!(mails[1].text.trim_end(), "");
}

#[tokio::test]
async fn typed_in_addresses_get_the_data_of_their_club_or_none() {
    let app = mail_app().await;
    let admin = app.create_admin().await;
    let (_, rsv) = club(&app, "Max Mustermann", "RSV Muster", None).await;

    let linked = send(
        &app,
        &admin,
        json!({
            "subject": "Info",
            "body": "Hallo {{ club_name }}",
            "extra_recipients": [{ "email": "kasse@rsv.test", "club_id": rsv }],
        }),
    )
    .await;
    let unlinked = send(
        &app,
        &admin,
        json!({
            "subject": "Info",
            "body": "Hallo zusammen",
            "extra_recipients": [{ "email": "presse@cup.test", "club_id": null }],
        }),
    )
    .await;
    let unlinked_with_club_data = send(
        &app,
        &admin,
        json!({
            "subject": "Info",
            "body": "Hallo {{ club_name }}",
            "club_ids": [rsv],
            "extra_recipients": [{ "email": "presse@cup.test" }],
        }),
    )
    .await;
    let invalid = send(
        &app,
        &admin,
        json!({
            "subject": "Info",
            "body": "Text",
            "extra_recipients": [{ "email": "presse at cup.test" }],
        }),
    )
    .await;
    let nobody = send(&app, &admin, json!({ "subject": "Info", "body": "Text" })).await;
    let no_subject = send(
        &app,
        &admin,
        json!({ "subject": " ", "body": "Text", "club_ids": [rsv] }),
    )
    .await;

    assert_eq!(linked.status, StatusCode::OK, "{:?}", linked.body);
    assert_eq!(unlinked.status, StatusCode::OK, "{:?}", unlinked.body);
    assert_refused(&unlinked_with_club_data, "presse@cup.test");
    assert_refused(&invalid, "presse at cup.test");
    assert_refused(&nobody, "Empfänger");
    assert_refused(&no_subject, "Betreff");
    let mails = app.mails().await;
    assert_eq!(mails.len(), 2, "{mails:#?}");
    assert_eq!(mails[0].to, ["kasse@rsv.test"]);
    assert_eq!(mails[0].text.trim_end(), "Hallo RSV Muster");
    assert_eq!(mails[1].to, ["presse@cup.test"]);
    assert_eq!(mails[1].text.trim_end(), "Hallo zusammen");
}

async fn sent_mails(app: &TestApp, cookie: &str) -> Vec<Value> {
    let response = app.get("/api/query/list_club_mails", Some(cookie)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body.as_array().unwrap().clone()
}

#[tokio::test]
async fn sent_mails_are_kept_as_written_and_as_each_recipient_got_them() {
    let app = mail_app().await;
    let admin = app.create_admin().await;
    let jury_lead = app.cookie_for(app.create_user_with_roles("Jo Jury", &["jury_lead"]).await);
    let (_, rsv) = club(&app, "Max Mustermann", "RSV Muster", None).await;

    let response = send(
        &app,
        &admin,
        json!({
            "subject": "Info für {{ club_name }}",
            "body": "Hallo {{ contact_name }}",
            "club_ids": [rsv],
            "extra_recipients": [{ "email": "presse@cup.test", "club_id": rsv }],
        }),
    )
    .await;

    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let log = sent_mails(&app, &jury_lead).await;
    assert_eq!(log.len(), 1);
    assert_eq!(response.body, log[0]);
    let mail = &log[0];
    assert_eq!(mail["sender_name"], "Admin");
    assert_eq!(mail["subject"], "Info für {{ club_name }}");
    assert_eq!(mail["body"], "Hallo {{ contact_name }}");
    let recipients = mail["mails"].as_array().unwrap();
    assert_eq!(recipients.len(), 2);
    for (recipient, to) in recipients
        .iter()
        .zip(["max.mustermann@cup.test", "presse@cup.test"])
    {
        assert_eq!(recipient["to"], json!([to]));
        assert_eq!(recipient["club_name"], "RSV Muster");
        assert_eq!(recipient["subject"], "Info für RSV Muster");
        assert_eq!(recipient["body"], "Hallo Max Mustermann");
        assert_eq!(recipient["status"], "sent");
        assert_eq!(recipient["error"], Value::Null);
    }
}

#[tokio::test]
async fn a_mail_that_fails_is_kept_with_its_error_and_the_others_still_go_out() {
    let db = common::in_memory_migrated().await;
    // Nothing listens on the app's mail server: every mail fails.
    let app = TestApp::new(db).await;
    let admin = app.create_admin().await;
    let (_, rsv) = club(&app, "Max Mustermann", "RSV Muster", None).await;
    let (_, tv) = club(&app, "Lea Beispiel", "TV Beispiel", None).await;

    let response = send(
        &app,
        &admin,
        json!({ "subject": "Info", "body": "Text", "club_ids": [rsv, tv] }),
    )
    .await;

    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let recipients = response.body["mails"].as_array().unwrap();
    assert_eq!(recipients.len(), 2);
    for recipient in recipients {
        assert_eq!(recipient["status"], "failed");
        assert!(!recipient["error"].as_str().unwrap().is_empty());
    }
}

#[tokio::test]
async fn only_senders_read_the_sent_mails() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let check_in = app.cookie_for(app.create_user_with_roles("Cem Check", &["check_in"]).await);

    let response = app.get("/api/query/list_club_mails", Some(&check_in)).await;

    assert_eq!(response.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_preview_renders_every_mail_without_sending_any() {
    let app = mail_app().await;
    let admin = app.create_admin().await;
    let (_, rsv) = club(
        &app,
        "Max Mustermann",
        "RSV Muster",
        Some(("Erika Muster", "erika@cup.test")),
    )
    .await;
    let mail = |body: &str| {
        json!({
            "subject": "Info",
            "body": body,
            "club_ids": [rsv],
            "extra_recipients": [{ "email": "presse@cup.test", "club_id": rsv }],
        })
    };

    let preview = app
        .post(
            "/api/command/preview_club_mail",
            mail("Hallo {{ club_name }}"),
            Some(&admin),
        )
        .await;
    let typo = app
        .post(
            "/api/command/preview_club_mail",
            mail("Hallo {{ club }}"),
            Some(&admin),
        )
        .await;

    assert_eq!(preview.status, StatusCode::OK, "{:?}", preview.body);
    assert_eq!(
        preview.body,
        json!({
            "reply_to": CONTACT_EMAIL,
            "mails": [
                {
                    "club_id": rsv,
                    "club_name": "RSV Muster",
                    "to": ["max.mustermann@cup.test", "erika@cup.test"],
                    "subject": "Info",
                    "body": "Hallo RSV Muster",
                },
                {
                    "club_id": rsv,
                    "club_name": "RSV Muster",
                    "to": ["presse@cup.test"],
                    "subject": "Info",
                    "body": "Hallo RSV Muster",
                },
            ],
        })
    );
    assert_refused(&typo, "club");
    assert!(app.mails().await.is_empty());
    assert!(sent_mails(&app, &admin).await.is_empty());
}

async fn mail_options(app: &TestApp, cookie: &str) -> Value {
    let response = app
        .get("/api/query/get_club_mail_options", Some(cookie))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body
}

fn variable_names(options: &Value) -> Vec<&str> {
    options["variables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn the_mail_form_gets_the_clubs_and_the_variables_the_sender_may_use() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;
    let treasurer = app.cookie_for(
        app.create_user_with_roles("Kim Kasse", &["treasurer"])
            .await,
    );
    let jury_lead = app.cookie_for(app.create_user_with_roles("Jo Jury", &["jury_lead"]).await);
    let (owner, rsv) = club(
        &app,
        "Max Mustermann",
        "RSV Muster",
        Some(("Erika Muster", "erika@cup.test")),
    )
    .await;
    let (_, tv) = club(&app, "Lea Beispiel", "TV Beispiel", None).await;
    app.add_single_act(&owner, rsv, "Lea").await;

    let for_treasurer = mail_options(&app, &treasurer).await;
    let for_jury_lead = mail_options(&app, &jury_lead).await;

    assert_eq!(
        for_treasurer["clubs"],
        json!([
            {
                "id": rsv,
                "name": "RSV Muster",
                "contact_name": "Max Mustermann",
                "contact_email": "max.mustermann@cup.test",
                "representative_name": "Erika Muster",
                "representative_email": "erika@cup.test",
                "open_amount": 43.0,
                "missing_deployments": 4,
                "missing_music": 1,
            },
            {
                "id": tv,
                "name": "TV Beispiel",
                "contact_name": "Lea Beispiel",
                "contact_email": "lea.beispiel@cup.test",
                "representative_name": null,
                "representative_email": null,
                "open_amount": 0.0,
                "missing_deployments": 0,
                "missing_music": 0,
            },
        ])
    );
    let rsv_for_jury_lead = &for_jury_lead["clubs"][0];
    assert_eq!(rsv_for_jury_lead["open_amount"], Value::Null);
    assert_eq!(rsv_for_jury_lead["missing_deployments"], 4);
    assert!(variable_names(&for_treasurer).contains(&"open_amount"));
    assert!(!variable_names(&for_jury_lead).contains(&"open_amount"));
    assert!(variable_names(&for_jury_lead).contains(&"missing_music"));
    assert!(
        for_treasurer["variables"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| !v["description"].as_str().unwrap().is_empty())
    );
}

#[tokio::test]
async fn without_the_jury_registration_there_are_no_missing_deployments() {
    let db = common::in_memory_migrated().await;
    let app = TestApp::without_jury_registration(db).await;
    let admin = app.create_admin().await;
    let (_, rsv) = club(&app, "Max Mustermann", "RSV Muster", None).await;

    let options = mail_options(&app, &admin).await;
    let response = app
        .post(
            "/api/command/preview_club_mail",
            json!({ "subject": "Jury", "body": "{{ missing_deployments }}", "club_ids": [rsv] }),
            Some(&admin),
        )
        .await;

    assert_eq!(options["clubs"][0]["missing_deployments"], Value::Null);
    assert!(!variable_names(&options).contains(&"missing_deployments"));
    assert_refused(&response, "missing_deployments");
}

#[tokio::test]
async fn an_address_gets_each_mail_once() {
    let app = mail_app().await;
    let admin = app.create_admin().await;
    let (_, rsv) = club(&app, "Max Mustermann", "RSV Muster", None).await;

    let response = send(
        &app,
        &admin,
        json!({
            "subject": "Info",
            "body": "Hallo {{ club_name }}",
            "club_ids": [rsv, rsv],
            "extra_recipients": [
                { "email": "Max.Mustermann@cup.test", "club_id": rsv },
                { "email": "presse@cup.test", "club_id": rsv },
                { "email": "PRESSE@cup.test", "club_id": rsv },
            ],
        }),
    )
    .await;

    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let to: Vec<_> = app.mails().await.into_iter().map(|m| m.to).collect();
    assert_eq!(to, [["max.mustermann@cup.test"], ["presse@cup.test"]]);
}

#[tokio::test]
async fn a_club_with_an_invalid_address_stops_the_mail_before_anything_is_sent() {
    let app = mail_app().await;
    let admin = app.create_admin().await;
    let (_, rsv) = club(&app, "Max Mustermann", "RSV Muster", None).await;
    let (_, tv) = club(&app, "Lea Beispiel", "TV Beispiel", None).await;
    sqlx::query("UPDATE clubs SET responsible_name = 'Erika Muster', responsible_email = 'erika@@cup.test' WHERE id = ?")
        .bind(tv)
        .execute(&app.db)
        .await
        .unwrap();

    let response = send(
        &app,
        &admin,
        json!({ "subject": "Info", "body": "Text", "club_ids": [rsv, tv] }),
    )
    .await;

    assert_refused(&response, "TV Beispiel");
    assert!(app.mails().await.is_empty());
}
