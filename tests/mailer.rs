//! Integration tests for sending mails over SMTP to Mailpit.

mod common;

use common::mailpit::{FROM_ADDRESS, Mailpit};
use einrad_server::mailer::Mailer;

#[tokio::test]
async fn creates_transport_from_smtp_url_for_local_mail_services() {
    let _mailer = Mailer::new(
        "smtp://localhost:1025",
        "noreply@localhost",
        "test-only",
        FROM_ADDRESS,
    );
}

#[tokio::test]
async fn sends_plain_text_mail() {
    let mailpit = Mailpit::start().await;

    mailpit
        .mailer()
        .send_text("anna@cup.test", "Hallo", "Erste Zeile\nZweite Zeile")
        .await
        .unwrap();

    let mails = mailpit.mails().await;
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].from, FROM_ADDRESS);
    assert_eq!(mails[0].to, vec!["anna@cup.test"]);
    assert_eq!(mails[0].subject, "Hallo");
    assert_eq!(mails[0].text.trim_end(), "Erste Zeile\nZweite Zeile");
}

#[tokio::test]
async fn non_ascii_subject_and_body_survive_encoding() {
    let mailpit = Mailpit::start().await;
    let subject = "Freestyle Cup NRW - Email bestätigen";
    let body = "Grüße an Jürgen & Zoë: äöüß € 🚲\nhttps://cup.test/verify_email?token=abc";

    mailpit
        .mailer()
        .send_text("juergen@cup.test", subject, body)
        .await
        .unwrap();

    let mails = mailpit.mails().await;
    assert_eq!(mails[0].subject, subject);
    assert_eq!(mails[0].text.trim_end(), body);
    assert_eq!(mails[0].token(), "abc");
}

#[tokio::test]
async fn mails_are_delivered_in_order() {
    let mailpit = Mailpit::start().await;
    let mailer = mailpit.mailer();

    for i in 0..3 {
        mailer
            .send_text(&format!("user{i}@cup.test"), &format!("Mail {i}"), "Text")
            .await
            .unwrap();
    }

    let subjects: Vec<_> = mailpit
        .mails()
        .await
        .into_iter()
        .map(|m| m.subject)
        .collect();
    assert_eq!(subjects, ["Mail 0", "Mail 1", "Mail 2"]);
}

#[tokio::test]
async fn unreachable_server_returns_error() {
    let mailer = Mailer::from_url("smtp://127.0.0.1:1", FROM_ADDRESS).unwrap();
    assert!(
        mailer
            .send_text("anna@cup.test", "Hallo", "Text")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn an_invalid_address_is_an_error_naming_it() {
    let mailer = Mailer::from_url("smtp://127.0.0.1:1", FROM_ADDRESS).unwrap();
    let to = mailer.send_text("not an address", "Hallo", "Text").await;
    let from = Mailer::from_url("smtp://127.0.0.1:1", "noreply")
        .unwrap()
        .send_text("anna@cup.test", "Hallo", "Text")
        .await;

    assert_eq!(
        to.unwrap_err().to_string(),
        "Ungültige Adresse: not an address"
    );
    assert_eq!(
        from.unwrap_err().to_string(),
        "Ungültige Absenderadresse: noreply"
    );
}
