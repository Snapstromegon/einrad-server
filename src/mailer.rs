use lettre::{
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor, message::header::ContentType,
    transport::smtp::authentication::Credentials,
};

pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from_address: String,
}

impl Mailer {
    /// `server` can be a relay hostname (using `username` and `password` for
    /// SMTP authentication) or an `smtp://`/`smtps://` URL. Include URL
    /// credentials in the URL when required; `from_address` is always the
    /// sender address.
    pub fn new(server: &str, username: &str, password: &str, from_address: &str) -> Self {
        let transport = if server.starts_with("smtp://") || server.starts_with("smtps://") {
            AsyncSmtpTransport::<Tokio1Executor>::from_url(server)
                .expect("invalid SMTP server configuration")
                .build()
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::relay(server)
                .expect("invalid SMTP server configuration")
                .credentials(Credentials::new(username.to_owned(), password.to_owned()))
                .build()
        };
        Self {
            transport,
            from_address: from_address.to_owned(),
        }
    }

    /// Creates a mailer from an SMTP connection URL.
    ///
    /// Supports the URL formats of [`AsyncSmtpTransport::from_url`], e.g.
    /// `smtp://localhost:1025` for an unencrypted local server like Mailpit or
    /// `smtps://user:password@mail.example.com` for implicit TLS. No connection
    /// is opened until the first mail is sent, but it must be called within a
    /// Tokio runtime.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// use einrad_server::mailer::Mailer;
    ///
    /// let mailer = Mailer::from_url("smtp://localhost:1025", "noreply@example.com");
    /// assert!(mailer.is_ok());
    /// assert!(Mailer::from_url("not a url", "noreply@example.com").is_err());
    /// # }
    /// ```
    pub fn from_url(url: &str, from_address: &str) -> Result<Self, lettre::transport::smtp::Error> {
        Ok(Self {
            transport: AsyncSmtpTransport::<Tokio1Executor>::from_url(url)?.build(),
            from_address: from_address.to_owned(),
        })
    }

    pub async fn send_text(
        &self,
        to: &str,
        subject: &str,
        body: &str,
    ) -> Result<(), lettre::transport::smtp::Error> {
        let email = lettre::Message::builder()
            .from(self.from_address.parse().unwrap())
            .to(to.parse().unwrap())
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())
            .unwrap();

        let result = self.transport.send(email).await;
        crate::metrics::metrics().mail_sent(result.is_ok());
        result?;
        Ok(())
    }
}
