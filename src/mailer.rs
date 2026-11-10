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
    ///
    /// # Panics
    /// If the server is invalid; see [`Mailer::try_new`].
    pub fn new(server: &str, username: &str, password: &str, from_address: &str) -> Self {
        Self::try_new(server, username, password, from_address)
            .expect("invalid SMTP server configuration")
    }

    /// Like [`Mailer::new`], but an invalid server is an error. No connection
    /// is opened; it must be called within a Tokio runtime.
    pub fn try_new(
        server: &str,
        username: &str,
        password: &str,
        from_address: &str,
    ) -> Result<Self, lettre::transport::smtp::Error> {
        let transport = if server.starts_with("smtp://") || server.starts_with("smtps://") {
            AsyncSmtpTransport::<Tokio1Executor>::from_url(server)?.build()
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::relay(server)?
                .credentials(Credentials::new(username.to_owned(), password.to_owned()))
                .build()
        };
        Ok(Self {
            transport,
            from_address: from_address.to_owned(),
        })
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

    /// Sends one plain text mail to `to`.
    pub async fn send_text(&self, to: &str, subject: &str, body: &str) -> Result<(), MailError> {
        self.send(&TextMail {
            to: &[to],
            reply_to: None,
            subject,
            body,
        })
        .await
    }

    /// Sends one plain text mail to every address in `to`.
    pub async fn send(&self, mail: &TextMail<'_>) -> Result<(), MailError> {
        let from = self
            .from_address
            .parse()
            .map_err(|_| MailError::FromAddress(self.from_address.clone()))?;
        let mut builder = lettre::Message::builder()
            .from(from)
            .subject(mail.subject)
            .header(ContentType::TEXT_PLAIN);
        for to in mail.to {
            builder = builder.to(mailbox(to)?);
        }
        if let Some(reply_to) = mail.reply_to {
            builder = builder.reply_to(mailbox(reply_to)?);
        }
        let email = builder.body(mail.body.to_string())?;

        let result = self.transport.send(email).await;
        crate::metrics::metrics().mail_sent(result.is_ok());
        result?;
        Ok(())
    }
}

fn mailbox(address: &str) -> Result<lettre::message::Mailbox, MailError> {
    address
        .parse()
        .map_err(|_| MailError::Address(address.to_string()))
}

/// A plain text mail for [`Mailer::send`].
#[derive(Debug, Clone, Copy)]
pub struct TextMail<'a> {
    /// Everyone the mail is addressed to; they see each other.
    pub to: &'a [&'a str],
    /// Where answers go instead of the sender address.
    pub reply_to: Option<&'a str>,
    pub subject: &'a str,
    pub body: &'a str,
}

/// Why a mail wasn't sent. The messages are German.
#[derive(Debug, thiserror::Error)]
pub enum MailError {
    /// A recipient or Reply-To that isn't an address.
    #[error("Ungültige Adresse: {0}")]
    Address(String),
    /// The configured sender address (the SMTP user) isn't an address.
    #[error("Ungültige Absenderadresse: {0}")]
    FromAddress(String),
    /// The message couldn't be put together, e.g. without a recipient.
    #[error("Die Mail lässt sich nicht erstellen: {0}")]
    Message(#[from] lettre::error::Error),
    /// The mail server couldn't be reached or refused the mail.
    #[error(transparent)]
    Smtp(#[from] lettre::transport::smtp::Error),
}
