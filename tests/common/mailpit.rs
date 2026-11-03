//! Mailpit SMTP server running in a container, used to capture sent mails.
//!
//! Requires a Docker-compatible container runtime. For Podman, point
//! `DOCKER_HOST` at its socket, e.g.
//! `DOCKER_HOST=unix:///run/user/$UID/podman/podman.sock`.

use einrad_server::mailer::Mailer;
use serde_json::Value;
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

const IMAGE: &str = "docker.io/axllent/mailpit";
const TAG: &str = "v1.31.4";
const SMTP_PORT: u16 = 1025;
const HTTP_PORT: u16 = 8025;

pub const FROM_ADDRESS: &str = "noreply@cup.test";

pub struct Mailpit {
    // Keeps the container alive; it is removed when dropped.
    _container: ContainerAsync<GenericImage>,
    smtp_url: String,
    api_url: String,
    http: reqwest::Client,
}

/// A mail as received and decoded by Mailpit.
#[derive(Debug)]
pub struct Mail {
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub text: String,
}

impl Mail {
    /// Returns the first `http(s)://` link in the mail body.
    pub fn link(&self) -> &str {
        let start = self.text.find("http").expect("mail contains a link");
        let rest = &self.text[start..];
        rest.split_whitespace().next().unwrap()
    }

    /// Returns the `token` query parameter of the first link in the mail body.
    pub fn token(&self) -> String {
        let link = url::Url::parse(self.link()).unwrap();
        link.query_pairs()
            .find(|(key, _)| key == "token")
            .expect("link contains a token")
            .1
            .into_owned()
    }
}

impl Mailpit {
    pub async fn start() -> Self {
        // Host ports are chosen up front instead of letting the runtime assign
        // them: Podman reports bindings with an empty `HostIp`, which
        // testcontainers cannot map back to an IPv4 port. Retry in case a
        // parallel test grabbed the same port in the meantime.
        let mut last_error = None;
        for _ in 0..3 {
            let (smtp_port, http_port) = (free_port(), free_port());
            match GenericImage::new(IMAGE, TAG)
                .with_wait_for(WaitFor::message_on_stdout("accessible via"))
                .with_mapped_port(smtp_port, SMTP_PORT.tcp())
                .with_mapped_port(http_port, HTTP_PORT.tcp())
                .start()
                .await
            {
                Ok(container) => {
                    // reqwest uses rustls without a crypto provider of its own.
                    let _ = rustls::crypto::ring::default_provider().install_default();
                    return Self {
                        _container: container,
                        smtp_url: format!("smtp://127.0.0.1:{smtp_port}"),
                        api_url: format!("http://127.0.0.1:{http_port}/api/v1"),
                        http: reqwest::Client::new(),
                    };
                }
                Err(error) => last_error = Some(error),
            }
        }
        panic!(
            "failed to start Mailpit container; is Docker (or Podman's socket) available? {:?}",
            last_error.unwrap()
        );
    }

    pub fn mailer(&self) -> Mailer {
        Mailer::new(&self.smtp_url, "test", "test", FROM_ADDRESS)
    }

    async fn get_json(&self, path: &str) -> Value {
        self.http
            .get(format!("{}{path}", self.api_url))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    /// Returns all received mails, oldest first.
    pub async fn mails(&self) -> Vec<Mail> {
        let list = self.get_json("/messages").await;
        let mut mails = Vec::new();
        // Mailpit lists the newest message first.
        for summary in list["messages"].as_array().unwrap().iter().rev() {
            let id = summary["ID"].as_str().unwrap();
            let message = self.get_json(&format!("/message/{id}")).await;
            mails.push(Mail {
                from: message["From"]["Address"].as_str().unwrap().to_string(),
                to: message["To"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|to| to["Address"].as_str().unwrap().to_string())
                    .collect(),
                subject: message["Subject"].as_str().unwrap().to_string(),
                // SMTP transfers lines with CRLF endings.
                text: message["Text"].as_str().unwrap().replace("\r\n", "\n"),
            });
        }
        mails
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
