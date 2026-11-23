//! Passwords and tokens sent to the server must never reach the log output,
//! not even through the span fields `#[instrument]` records for a handler.

mod common;

use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use common::{TestApp, email_for};
use serde_json::json;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Captured;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test]
async fn login_with_unknown_email_does_not_log_the_password() {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(captured.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let db = common::in_memory_migrated().await;
    let app = TestApp::new(db).await;

    let response = app
        .post(
            "/api/command/login",
            json!({ "email": email_for("Max Müller"), "password": "geheim-123" }),
            None,
        )
        .await;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    let logs = captured.text();
    assert!(
        logs.contains("Failed to find user"),
        "the handler's error event was not captured: {logs}"
    );
    assert!(
        !logs.contains("geheim-123"),
        "password leaked into logs: {logs}"
    );
}
