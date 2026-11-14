//! Club mails (Rundmail, #257): the data a mail is rendered with, the log
//! of sent mails and the [`ClubMailsRepo`] that reads and keeps them. The
//! templating itself lives in the application.

use time::OffsetDateTime;

use crate::{
    DbError,
    bookings::round_cents,
    ids::{ClubId, ClubMailId, UserId},
};

/// The data of one club a mail can use.
#[derive(Debug, Clone)]
pub struct ClubData {
    pub id: ClubId,
    pub name: String,
    pub code: String,
    pub contact_name: String,
    pub contact_email: String,
    /// The club representative, also when it's the club contact.
    pub representative_name: String,
    pub representative_email: String,
    /// What the club owes, in €.
    pub amount_due: f64,
    /// The sum of its payments, in €.
    pub paid: f64,
    /// Missing jury deployments; 0 without the jury registration.
    pub missing_deployments: i64,
    /// The acts the club looks after that have no music yet.
    pub missing_music: Vec<ActWithoutMusic>,
}

/// An act without music, as the templates see it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ActWithoutMusic {
    /// The group's name, or the riders' names of a single or pair.
    pub name: String,
    /// "Einzelkür", "Paarkür", "Kleingruppe" or "Großgruppe".
    #[serde(rename = "type")]
    pub act_type: &'static str,
    /// The act's title (Kürtitel); empty if it has none.
    pub title: String,
}

impl ClubData {
    /// Where the club's mail goes: the club contact and the club
    /// representative, each address once.
    pub fn recipients(&self) -> Vec<String> {
        let mut recipients = vec![self.contact_email.clone()];
        if self.has_own_representative() {
            recipients.push(self.representative_email.clone());
        }
        recipients
    }

    /// Whether the club named a club representative other than its contact.
    pub fn has_own_representative(&self) -> bool {
        !self
            .representative_email
            .eq_ignore_ascii_case(&self.contact_email)
    }

    /// What is still to pay, in €; negative when overpaid.
    pub fn open_amount(&self) -> f64 {
        round_cents(self.amount_due - self.paid)
    }
}

/// One mail as it goes out.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct RenderedMail {
    /// The club whose data it was rendered with; `null` for a typed-in
    /// address without a club.
    pub club_id: Option<ClubId>,
    pub club_name: Option<String>,
    /// Everyone it's addressed to, in one mail.
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
}

/// What became of one mail.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    sqlx::Type,
    utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum DeliveryStatus {
    /// Not handed to the mail server yet, e.g. the server stopped meanwhile.
    Pending,
    Sent,
    Failed,
}

/// A club mail in the log: as written, with every mail as it went out.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct SentClubMail {
    pub id: ClubMailId,
    #[serde(with = "time::serde::rfc3339")]
    pub sent_at: OffsetDateTime,
    pub sender_name: String,
    /// The templates as written.
    pub subject: String,
    pub body: String,
    pub reply_to: Option<String>,
    pub mails: Vec<SentMail>,
}

/// One mail of a club mail as it went out.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct SentMail {
    /// The row to keep the delivery status in.
    #[serde(skip)]
    pub id: i64,
    /// The club whose data it was rendered with, and its name at the time.
    pub club_id: Option<ClubId>,
    pub club_name: Option<String>,
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
    pub status: DeliveryStatus,
    /// Why it failed.
    pub error: Option<String>,
}

#[async_trait::async_trait]
pub trait ClubMailsRepo {
    /// The clubs `ids`, in that order; `None` if one doesn't exist.
    /// `jury_registration`: whether the feature is on.
    async fn load_mail_clubs(
        &self,
        ids: &[ClubId],
        jury_registration: bool,
    ) -> Result<Option<Vec<ClubData>>, DbError>;

    /// Every club, by name.
    async fn load_all_mail_clubs(&self, jury_registration: bool) -> Result<Vec<ClubData>, DbError>;

    /// Where answers to club mails go: the competition's contact email, if
    /// it isn't blank.
    async fn mail_reply_to(&self) -> Result<Option<String>, DbError>;

    /// Keeps a club mail before anything goes out: every mail as pending.
    /// Returns the log entry, mails in the order of `mails`.
    async fn record_club_mail(
        &self,
        author_id: UserId,
        author_name: &str,
        subject: &str,
        body: &str,
        reply_to: Option<&str>,
        mails: Vec<RenderedMail>,
    ) -> Result<SentClubMail, DbError>;

    /// Keeps whether one mail (a [`SentMail::id`]) went out.
    async fn set_mail_status(
        &self,
        mail_id: i64,
        status: DeliveryStatus,
        error: Option<&str>,
    ) -> Result<(), DbError>;

    /// Every club mail sent, newest first.
    async fn list_club_mails(&self) -> Result<Vec<SentClubMail>, DbError>;
}
