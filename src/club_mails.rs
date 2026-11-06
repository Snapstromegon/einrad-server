//! Club mails (Rundmail, #257): the organizer team writes one mail as a
//! template and sends it to selected clubs, each club's mail rendered with
//! its own data and addressed to its club contact and club representative.
//!
//! Templates are MiniJinja (variables, loops and conditionals) in a sandbox:
//! no includes or imports, only the variables in [`VARIABLES`], and a fuel
//! limit. A template is checked before anything is sent.

use std::collections::{BTreeMap, HashMap, HashSet};

use minijinja::{
    AutoEscape, Environment, UndefinedBehavior,
    value::{Serde, Value},
};
use time::OffsetDateTime;
use tracing::warn;
use uuid::Uuid;

use crate::{
    act_type::ActType,
    bookings::{paid_by_club, round_cents},
    competition::Competition,
    fees::fees_by_club,
    http_server::{HttpError, extractor::auth::Auth},
    mailer::{Mailer, TextMail},
    reloadable_sqlite::begin_write,
    roles::Permission,
    utils::club_code,
};

/// The data of one club a mail can use.
#[derive(Debug, Clone)]
pub struct ClubData {
    pub id: Uuid,
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

/// The acts `club_id` looks after that have no music: its singles and pairs
/// and the groups it created, without withdrawn ones; by type, then name.
async fn acts_without_music(
    db: &sqlx::SqlitePool,
    club_id: Uuid,
) -> sqlx::Result<Vec<ActWithoutMusic>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            acts.act_type as "act_type: ActType",
            acts.name as title,
            COALESCE(
                acts.group_name,
                (
                    SELECT group_concat(rider.firstname || ' ' || rider.lastname, ' & ')
                    FROM act_participants
                    JOIN rider ON rider.id = act_participants.rider_id
                    WHERE act_participants.act_id = acts.id
                ),
                ''
            ) as "name!: String"
        FROM acts
        WHERE acts.song_file IS NULL
            AND acts.withdrawn_at IS NULL
            AND CASE WHEN acts.act_type IN ('group_small', 'group_large')
                THEN acts.owner_club_id = ?1
                ELSE EXISTS (
                    SELECT 1 FROM act_participants
                    JOIN rider ON rider.id = act_participants.rider_id
                    WHERE act_participants.act_id = acts.id AND rider.club_id = ?1
                )
            END
        "#,
        club_id
    )
    .fetch_all(db)
    .await?;
    let mut acts: Vec<_> = rows
        .into_iter()
        .map(|row| {
            (
                row.act_type,
                ActWithoutMusic {
                    name: row.name,
                    act_type: row.act_type.german_name(),
                    title: row.title,
                },
            )
        })
        .collect();
    let order = [
        ActType::Single,
        ActType::Pair,
        ActType::GroupSmall,
        ActType::GroupLarge,
    ];
    acts.sort_by_key(|(act_type, act)| {
        (
            order.iter().position(|t| t == act_type),
            act.name.to_lowercase(),
        )
    });
    Ok(acts.into_iter().map(|(_, act)| act).collect())
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
    fn has_own_representative(&self) -> bool {
        !self
            .representative_email
            .eq_ignore_ascii_case(&self.contact_email)
    }

    /// What is still to pay, in €; negative when overpaid.
    pub fn open_amount(&self) -> f64 {
        round_cents(self.amount_due - self.paid)
    }

    /// A made-up club, to try a template before sending it.
    fn sample() -> Self {
        let id = Uuid::nil();
        Self {
            id,
            name: "RSV Muster".to_string(),
            code: club_code(&id),
            contact_name: "Max Mustermann".to_string(),
            contact_email: "max@example.org".to_string(),
            representative_name: "Max Mustermann".to_string(),
            representative_email: "max@example.org".to_string(),
            amount_due: 45.0,
            paid: 15.0,
            missing_deployments: 2,
            missing_music: vec![ActWithoutMusic {
                name: "Lea Muster".to_string(),
                act_type: ActType::Single.german_name(),
                title: "Sommerwind".to_string(),
            }],
        }
    }

    /// Every club, by name.
    pub async fn load_all(
        db: &sqlx::SqlitePool,
        jury_registration: bool,
    ) -> sqlx::Result<Vec<Self>> {
        let ids = sqlx::query_scalar!(r#"SELECT id as "id!: Uuid" FROM clubs ORDER BY name"#)
            .fetch_all(db)
            .await?;
        Ok(Self::load(db, &ids, jury_registration)
            .await?
            .unwrap_or_default())
    }

    /// The clubs `ids`, in that order; `None` if one doesn't exist.
    /// `jury_registration`: whether the feature is on, see `Features`.
    pub async fn load(
        db: &sqlx::SqlitePool,
        ids: &[Uuid],
        jury_registration: bool,
    ) -> sqlx::Result<Option<Vec<Self>>> {
        let competition = Competition::current(db).await?;
        let mut fees = fees_by_club(db, competition.as_ref(), jury_registration).await?;
        let paid = paid_by_club(db).await?;
        let mut clubs = Vec::with_capacity(ids.len());
        for &id in ids {
            let row = sqlx::query!(
                r#"
                SELECT
                    clubs.name,
                    users.name as contact_name,
                    users.email as contact_email,
                    COALESCE(clubs.responsible_name, users.name) as "representative_name!: String",
                    COALESCE(clubs.responsible_email, users.email) as "representative_email!: String"
                FROM clubs
                JOIN users ON users.id = clubs.owner_id
                WHERE clubs.id = ?
                "#,
                id
            )
            .fetch_optional(db)
            .await?;
            let Some(row) = row else {
                return Ok(None);
            };
            let fees = fees.remove(&id).unwrap_or_default();
            clubs.push(Self {
                id,
                name: row.name,
                code: club_code(&id),
                contact_name: row.contact_name,
                contact_email: row.contact_email,
                representative_name: row.representative_name,
                representative_email: row.representative_email,
                amount_due: fees.amount_due.total,
                paid: paid.get(&id).copied().unwrap_or(0.0),
                missing_deployments: fees.jury.map_or(0, |jury| jury.missing),
                missing_music: acts_without_music(db, id).await?,
            });
        }
        Ok(Some(clubs))
    }
}

/// Who may use a variable, besides holding the permission to send club
/// mails: the same rule as the club list's balances and jury obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Everyone,
    /// Money: with the permission to see club balances.
    Balances,
    /// The jury obligation: with the jury or the club balances.
    Jury,
}

/// What the sender of a mail may see of the clubs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sender {
    pub balances: bool,
    pub jury: bool,
}

impl Sender {
    pub fn of(auth: &Auth) -> Self {
        let balances = auth.can(Permission::ViewClubBalances);
        Self {
            balances,
            jury: balances || auth.can(Permission::ManageJury),
        }
    }

    pub fn may(self, access: Access) -> bool {
        match access {
            Access::Everyone => true,
            Access::Balances => self.balances,
            Access::Jury => self.jury,
        }
    }
}

/// A variable of the mail templates. A new variable is one more entry in
/// [`VARIABLES`].
pub struct Variable {
    pub name: &'static str,
    /// What it holds, in German for the mail form.
    pub description: &'static str,
    pub access: Access,
    value: fn(&ClubData) -> Value,
}

/// Every variable the templates can use, in the order the mail form lists
/// them. Amounts are numbers in €; `{{ amount | euro }}` writes them like
/// "1.505,00 €".
pub const VARIABLES: &[Variable] = &[
    Variable {
        name: "club_name",
        description: "Name des Vereins",
        access: Access::Everyone,
        value: |club| Value::from(club.name.clone()),
    },
    Variable {
        name: "club_code",
        description: "Vereinscode für den Verwendungszweck, z. B. EV-K7Q3",
        access: Access::Everyone,
        value: |club| Value::from(club.code.clone()),
    },
    Variable {
        name: "contact_name",
        description: "Name der Kontaktperson des Vereins",
        access: Access::Everyone,
        value: |club| Value::from(club.contact_name.clone()),
    },
    Variable {
        name: "amount_due",
        description: "Betrag: Startgeld und Jurygebühr, ohne Erlasse (Zahl in €)",
        access: Access::Balances,
        value: |club| Value::from(club.amount_due),
    },
    Variable {
        name: "paid",
        description: "Gezahlt (Zahl in €)",
        access: Access::Balances,
        value: |club| Value::from(club.paid),
    },
    Variable {
        name: "open_amount",
        description: "Offen: Betrag minus Gezahlt, negativ bei Überzahlung (Zahl in €)",
        access: Access::Balances,
        value: |club| Value::from(club.open_amount()),
    },
    Variable {
        name: "missing_deployments",
        description: "Fehlende Juryeinsätze",
        access: Access::Jury,
        value: |club| Value::from(club.missing_deployments),
    },
    Variable {
        name: "missing_music",
        description: "Küren ohne Musik, jede mit name (Gruppe oder FahrerInnen), type (z. B. Einzelkür) und title (Kürtitel)",
        access: Access::Everyone,
        value: |club| Value::from(Serde(&club.missing_music)),
    },
];

/// The variables of `club` that `sender` may use, for a template.
fn context(club: &ClubData, sender: Sender) -> Value {
    let variables: BTreeMap<&str, Value> = VARIABLES
        .iter()
        .filter(|variable| sender.may(variable.access))
        .map(|variable| (variable.name, (variable.value)(club)))
        .collect();
    Value::from(variables)
}

/// `amount` in € the German way, e.g. "1.505,00 €"; the templates' `euro`
/// filter.
pub fn format_euro(amount: f64) -> String {
    let cents = (amount * 100.0).round() as i64;
    let euros = (cents.abs() / 100).to_string();
    let mut grouped = String::new();
    for (i, digit) in euros.chars().enumerate() {
        if i > 0 && (euros.len() - i).is_multiple_of(3) {
            grouped.push('.');
        }
        grouped.push(digit);
    }
    let sign = if cents < 0 { "-" } else { "" };
    format!("{sign}{grouped},{:02} €", cents.abs() % 100)
}

/// Instructions a template may run per mail, against endless loops.
const FUEL: u64 = 100_000;

/// The sandbox the templates run in: no loader (and no include or import
/// tags at all), plain text, and an error for anything undefined.
fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env.set_fuel(Some(FUEL));
    env.add_filter("euro", format_euro);
    env
}

/// A mail as written: subject and body are templates.
#[derive(Debug, Clone, serde::Deserialize, utoipa::ToSchema)]
pub struct ClubMailInput {
    pub subject: String,
    pub body: String,
    /// The clubs to write to: each gets one mail with its data, to its club
    /// contact and club representative.
    #[serde(default)]
    pub club_ids: Vec<Uuid>,
    /// Typed-in addresses, each getting a mail of its own.
    #[serde(default)]
    pub extra_recipients: Vec<ExtraRecipient>,
}

/// A typed-in address.
#[derive(Debug, Clone, serde::Deserialize, utoipa::ToSchema)]
pub struct ExtraRecipient {
    pub email: String,
    /// The club whose data its mail gets; without one, the template can't
    /// use any variable.
    #[serde(default)]
    pub club_id: Option<Uuid>,
}

/// One mail as it goes out.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct RenderedMail {
    /// The club whose data it was rendered with; `null` for a typed-in
    /// address without a club.
    pub club_id: Option<Uuid>,
    pub club_name: Option<String>,
    /// Everyone it's addressed to, in one mail.
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
}

/// A template that compiled, uses only variables its sender may use and
/// rendered for a sample club.
struct MailTemplate<'a> {
    subject: &'a str,
    body: &'a str,
    sender: Sender,
    /// The variables it uses, in the order of [`VARIABLES`].
    uses: Vec<&'static str>,
}

impl<'a> MailTemplate<'a> {
    /// Checks the template against what `sender` may use; the message is
    /// German.
    fn check(subject: &'a str, body: &'a str, sender: Sender) -> Result<Self, String> {
        let env = environment();
        let known: HashSet<&str> = VARIABLES
            .iter()
            .map(|v| v.name)
            .chain(env.globals().map(|(name, _)| name))
            .collect();
        let mut uses = HashSet::new();
        for (part, source) in [("Betreff", subject), ("Text", body)] {
            let template = env
                .template_from_str(source)
                .map_err(|e| template_error(part, &e))?;
            let undeclared = template.undeclared_variables(false);
            let mut unknown: Vec<_> = undeclared
                .iter()
                .filter(|name| !known.contains(name.as_str()))
                .map(String::as_str)
                .collect();
            unknown.sort_unstable();
            if !unknown.is_empty() {
                return Err(format!(
                    "Unbekannte Variable im {part}: {}",
                    unknown.join(", ")
                ));
            }
            let forbidden: Vec<_> = VARIABLES
                .iter()
                .filter(|v| !sender.may(v.access) && undeclared.contains(v.name))
                .map(|v| v.name)
                .collect();
            if !forbidden.is_empty() {
                return Err(format!(
                    "Keine Berechtigung für {} im {part}",
                    forbidden.join(", ")
                ));
            }
            uses.extend(undeclared);
        }
        let template = Self {
            subject,
            body,
            sender,
            uses: VARIABLES
                .iter()
                .map(|v| v.name)
                .filter(|name| uses.contains(*name))
                .collect(),
        };
        template.render(Some(&ClubData::sample()))?;
        Ok(template)
    }

    /// The subject (on one line) and the body, with the data of `club`.
    fn render(&self, club: Option<&ClubData>) -> Result<(String, String), String> {
        let env = environment();
        let context = club.map_or_else(
            || Value::from(BTreeMap::<&str, Value>::new()),
            |club| context(club, self.sender),
        );
        let render = |part: &str, source: &str| {
            env.render_str(source, context.clone())
                .map_err(|e| template_error(part, &e))
        };
        let subject = render("Betreff", self.subject)?;
        let subject = subject.split_whitespace().collect::<Vec<_>>().join(" ");
        Ok((subject, render("Text", self.body)?))
    }
}

/// A German message for an error in the `part` of a template.
fn template_error(part: &str, error: &minijinja::Error) -> String {
    let line = error
        .line()
        .map(|line| format!(", Zeile {line}"))
        .unwrap_or_default();
    let detail = error
        .detail()
        .map(|detail| format!(": {detail}"))
        .unwrap_or_default();
    format!("Fehler im {part}{line}: {}{detail}", error.kind())
}

/// Every mail of a club mail, ready to send.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct PreparedMails {
    /// Where answers go: the competition's contact email. Without one,
    /// answers go to the sender address, which nobody reads.
    pub reply_to: Option<String>,
    pub mails: Vec<RenderedMail>,
}

/// Checks `input` and renders every mail, before anything is sent: one per
/// club, then one per typed-in address. `jury_registration`: whether the
/// feature is on, see `Features`.
pub async fn prepare(
    db: &sqlx::SqlitePool,
    input: &ClubMailInput,
    sender: Sender,
    jury_registration: bool,
) -> Result<PreparedMails, HttpError> {
    let invalid = |message: String| HttpError::ErrorMessages(message);
    if input.subject.trim().is_empty() {
        return Err(invalid("Bitte einen Betreff eingeben.".to_string()));
    }
    if input.body.trim().is_empty() {
        return Err(invalid("Bitte einen Text eingeben.".to_string()));
    }
    if input.club_ids.is_empty() && input.extra_recipients.is_empty() {
        return Err(invalid(
            "Bitte mindestens einen Empfänger wählen: einen Verein oder eine Adresse.".to_string(),
        ));
    }
    let extra: Vec<_> = input
        .extra_recipients
        .iter()
        .map(|recipient| {
            let email = recipient.email.trim();
            email
                .parse::<lettre::Address>()
                .map(|_| (email.to_string(), recipient.club_id))
                .map_err(|_| invalid(format!("Ungültige Adresse: {email}")))
        })
        .collect::<Result<_, _>>()?;
    let template = MailTemplate::check(&input.subject, &input.body, sender).map_err(invalid)?;
    if let Some((email, _)) = extra.iter().find(|(_, club_id)| club_id.is_none())
        && !template.uses.is_empty()
    {
        return Err(invalid(format!(
            "{email} gehört zu keinem Verein, die Mail braucht aber Vereinsdaten ({}). \
             Ordne die Adresse einem Verein zu oder lass die Variablen weg.",
            template.uses.join(", ")
        )));
    }

    let mut club_ids: Vec<Uuid> = Vec::new();
    for id in input
        .club_ids
        .iter()
        .chain(extra.iter().filter_map(|(_, id)| id.as_ref()))
    {
        if !club_ids.contains(id) {
            club_ids.push(*id);
        }
    }
    let clubs: HashMap<Uuid, ClubData> = ClubData::load(db, &club_ids, jury_registration)
        .await?
        .ok_or(HttpError::NotFound)?
        .into_iter()
        .map(|club| (club.id, club))
        .collect();
    let mail = |club: Option<&ClubData>, to: Vec<String>| {
        let (subject, body) = template.render(club)?;
        Ok(RenderedMail {
            club_id: club.map(|club| club.id),
            club_name: club.map(|club| club.name.clone()),
            to,
            subject,
            body,
        })
    };
    let mut seen = HashSet::new();
    let mails = input
        .club_ids
        .iter()
        .filter(|id| seen.insert(**id))
        .map(|id| mail(Some(&clubs[id]), clubs[id].recipients()))
        .chain(
            extra
                .iter()
                .map(|(email, club_id)| mail(club_id.map(|id| &clubs[&id]), vec![email.clone()])),
        )
        .collect::<Result<_, String>>()
        .map_err(invalid)?;
    Ok(PreparedMails {
        reply_to: reply_to(db).await?,
        mails,
    })
}

/// A club as the mail form lists it, to choose the recipients.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct MailClub {
    pub id: Uuid,
    pub name: String,
    pub contact_name: String,
    pub contact_email: String,
    /// The club representative; `null` when it's the club contact.
    pub representative_name: Option<String>,
    pub representative_email: Option<String>,
    /// What is still to pay in €, negative when overpaid; only with club
    /// balances.
    pub open_amount: Option<f64>,
    /// Only with the jury or club balances, while the jury registration is
    /// on.
    pub missing_deployments: Option<i64>,
    /// How many of its acts have no music yet.
    pub missing_music: i64,
}

/// A variable the sender may use, for the mail form.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct VariableInfo {
    pub name: &'static str,
    pub description: &'static str,
}

/// What the mail form needs: the clubs and the variables the sender may use.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct ClubMailOptions {
    pub clubs: Vec<MailClub>,
    pub variables: Vec<VariableInfo>,
    /// Where answers go, see [`PreparedMails::reply_to`].
    pub reply_to: Option<String>,
}

/// The mail form's clubs and variables for `sender`. `jury_registration`:
/// whether the feature is on, see `Features`.
pub async fn options(
    db: &sqlx::SqlitePool,
    sender: Sender,
    jury_registration: bool,
) -> sqlx::Result<ClubMailOptions> {
    let clubs = ClubData::load_all(db, jury_registration)
        .await?
        .into_iter()
        .map(|club| {
            let own = club.has_own_representative();
            MailClub {
                open_amount: sender.balances.then(|| club.open_amount()),
                missing_deployments: (sender.jury && jury_registration)
                    .then_some(club.missing_deployments),
                missing_music: club.missing_music.len() as i64,
                representative_name: own.then(|| club.representative_name.clone()),
                representative_email: own.then(|| club.representative_email.clone()),
                id: club.id,
                name: club.name,
                contact_name: club.contact_name,
                contact_email: club.contact_email,
            }
        })
        .collect();
    Ok(ClubMailOptions {
        clubs,
        variables: VARIABLES
            .iter()
            .filter(|variable| sender.may(variable.access))
            .map(|variable| VariableInfo {
                name: variable.name,
                description: variable.description,
            })
            .collect(),
        reply_to: reply_to(db).await?,
    })
}

/// Where answers to club mails go: the competition's contact email.
async fn reply_to(db: &sqlx::SqlitePool) -> sqlx::Result<Option<String>> {
    Ok(Competition::current(db)
        .await?
        .and_then(|competition| competition.contact_email)
        .filter(|email| !email.trim().is_empty()))
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
    pub id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub sent_at: OffsetDateTime,
    pub sender_name: String,
    /// The templates as written.
    pub subject: String,
    pub body: String,
    pub reply_to: Option<String>,
    pub recipients: Vec<SentMail>,
}

/// One mail of a club mail as it went out.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct SentMail {
    #[serde(skip)]
    id: i64,
    /// The club whose data it was rendered with, and its name at the time.
    pub club_id: Option<Uuid>,
    pub club_name: Option<String>,
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
    pub status: DeliveryStatus,
    /// Why it failed.
    pub error: Option<String>,
}

/// Keeps `prepared`, a club mail `sender` sends, before it goes out: every
/// mail as pending. Returns the entry, in the order of `prepared`.
pub async fn record(
    db: &sqlx::SqlitePool,
    sender: &Auth,
    input: &ClubMailInput,
    prepared: &PreparedMails,
) -> sqlx::Result<SentClubMail> {
    let id = Uuid::now_v7();
    let sent_at = OffsetDateTime::now_utc();
    let mut tx = begin_write(db).await?;
    sqlx::query!(
        r#"
        INSERT INTO club_mails (id, sent_at, user_id, user_name, subject, body, reply_to)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
        id,
        sent_at,
        sender.user_id,
        sender.name,
        input.subject,
        input.body,
        prepared.reply_to,
    )
    .execute(&mut *tx)
    .await?;
    let mut recipients = Vec::with_capacity(prepared.mails.len());
    for mail in &prepared.mails {
        let to = serde_json::to_string(&mail.to).expect("addresses serialize");
        let recipient_id = sqlx::query_scalar!(
            r#"
            INSERT INTO club_mail_recipients (mail_id, club_id, club_name, recipients, subject, body)
            VALUES (?, ?, ?, ?, ?, ?)
            RETURNING id as "id!: i64"
            "#,
            id,
            mail.club_id,
            mail.club_name,
            to,
            mail.subject,
            mail.body,
        )
        .fetch_one(&mut *tx)
        .await?;
        recipients.push(SentMail {
            id: recipient_id,
            club_id: mail.club_id,
            club_name: mail.club_name.clone(),
            to: mail.to.clone(),
            subject: mail.subject.clone(),
            body: mail.body.clone(),
            status: DeliveryStatus::Pending,
            error: None,
        });
    }
    tx.commit().await?;
    Ok(SentClubMail {
        id,
        sent_at,
        sender_name: sender.name.clone(),
        subject: input.subject.clone(),
        body: input.body.clone(),
        reply_to: prepared.reply_to.clone(),
        recipients,
    })
}

/// Sends every pending mail of `mail` one by one and keeps whether it went
/// out; one that fails doesn't stop the others.
pub async fn send(
    db: &sqlx::SqlitePool,
    mailer: &Mailer,
    mail: &mut SentClubMail,
) -> sqlx::Result<()> {
    for recipient in &mut mail.recipients {
        let to: Vec<&str> = recipient.to.iter().map(String::as_str).collect();
        let result = mailer
            .send(&TextMail {
                to: &to,
                reply_to: mail.reply_to.as_deref(),
                subject: &recipient.subject,
                body: &recipient.body,
            })
            .await;
        if let Err(error) = &result {
            warn!("Club mail {} to {to:?} failed: {error}", mail.id);
        }
        recipient.status = if result.is_ok() {
            DeliveryStatus::Sent
        } else {
            DeliveryStatus::Failed
        };
        recipient.error = result.err().map(|error| error.to_string());
        sqlx::query!(
            "UPDATE club_mail_recipients SET status = ?, error = ? WHERE id = ?",
            recipient.status,
            recipient.error,
            recipient.id,
        )
        .execute(db)
        .await?;
    }
    Ok(())
}

/// Every club mail sent, newest first.
pub async fn log(db: &sqlx::SqlitePool) -> sqlx::Result<Vec<SentClubMail>> {
    let mails = sqlx::query!(
        r#"
        SELECT
            id as "id!: Uuid",
            sent_at as "sent_at!: OffsetDateTime",
            user_name,
            subject,
            body,
            reply_to
        FROM club_mails
        ORDER BY sent_at DESC, id DESC
        "#
    )
    .fetch_all(db)
    .await?;
    let recipients = sqlx::query!(
        r#"
        SELECT
            id as "id!: i64",
            mail_id as "mail_id!: Uuid",
            club_id as "club_id: Uuid",
            club_name,
            recipients,
            subject,
            body,
            status as "status!: DeliveryStatus",
            error
        FROM club_mail_recipients
        ORDER BY id
        "#
    )
    .fetch_all(db)
    .await?;
    let mut by_mail: HashMap<Uuid, Vec<SentMail>> = HashMap::new();
    for row in recipients {
        by_mail.entry(row.mail_id).or_default().push(SentMail {
            id: row.id,
            club_id: row.club_id,
            club_name: row.club_name,
            to: serde_json::from_str(&row.recipients).unwrap_or_default(),
            subject: row.subject,
            body: row.body,
            status: row.status,
            error: row.error,
        });
    }
    Ok(mails
        .into_iter()
        .map(|mail| SentClubMail {
            recipients: by_mail.remove(&mail.id).unwrap_or_default(),
            id: mail.id,
            sent_at: mail.sent_at,
            sender_name: mail.user_name,
            subject: mail.subject,
            body: mail.body,
            reply_to: mail.reply_to,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn euros_are_written_the_german_way() {
        assert_eq!(format_euro(0.0), "0,00 €");
        assert_eq!(format_euro(23.5), "23,50 €");
        assert_eq!(format_euro(1505.0), "1.505,00 €");
        assert_eq!(format_euro(1234567.891), "1.234.567,89 €");
        assert_eq!(format_euro(-5.0), "-5,00 €");
    }
}
