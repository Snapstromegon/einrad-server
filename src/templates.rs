//! Mail templates. Mails are plain text, so they are rendered without HTML
//! escaping (`escape = "none"`).

use askama::Template;

/// Email asking a newly registered user to confirm their address.
///
/// ```
/// use askama::Template;
/// use einrad_server::templates::VerifyMail;
///
/// let body = VerifyMail {
///     event_name: "Einradevent 2027",
///     name: "Alex",
///     verify_link: "https://example.com/verify_email?token=abc",
/// }
/// .render()
/// .unwrap();
/// assert!(body.starts_with("Hallo Alex,"));
/// assert!(body.contains("https://example.com/verify_email?token=abc"));
/// assert!(body.contains("Dein Einradevent 2027 Team"));
/// ```
#[derive(Template)]
#[template(path = "emails/verify-mail.j2", escape = "none")]
pub struct VerifyMail<'a> {
    /// The competition's name, see [`crate::competition::Competition::event_name`].
    pub event_name: &'a str,
    pub name: &'a str,
    pub verify_link: &'a str,
}

/// Email containing a password reset link.
#[derive(Template)]
#[template(path = "emails/password-reset-mail.j2", escape = "none")]
pub struct PasswordResetMail<'a> {
    pub event_name: &'a str,
    pub name: &'a str,
    pub reset_link: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_reset_mail_contains_name_and_link() {
        let body = PasswordResetMail {
            event_name: "Einradevent 2027",
            name: "Alex",
            reset_link: "https://example.com/reset_password?token=abc",
        }
        .render()
        .unwrap();
        assert!(body.starts_with("Hallo Alex,"));
        assert!(body.contains("https://example.com/reset_password?token=abc"));
    }

    #[test]
    fn mails_are_plain_text_without_html_escaping() {
        let body = VerifyMail {
            event_name: "Einradevent 2027",
            name: "Anna & Berta O'Brien",
            verify_link: "https://example.com/verify_email?token=abc",
        }
        .render()
        .unwrap();
        assert!(body.contains("Anna & Berta O'Brien"), "{body}");
    }

    #[test]
    fn event_names_are_not_html_escaped() {
        let body = PasswordResetMail {
            event_name: "Einrad & Co. <NRW>",
            name: "Alex",
            reset_link: "https://example.com/reset_password?token=abc&x=1",
        }
        .render()
        .unwrap();
        assert!(body.contains("Dein Einrad & Co. <NRW> Team"), "{body}");
        assert!(body.contains("token=abc&x=1"), "{body}");
    }
}
