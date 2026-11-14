//! Club mails (Rundmail, #257): the organizer team writes one mail as a
//! template and sends it to selected clubs, each club's mail rendered with
//! its own data and addressed to its club contact and club representative.
//!
//! Templates are MiniJinja (variables, loops and conditionals) in a sandbox:
//! no includes or imports, only the variables in [`VARIABLES`], and a fuel
//! limit. A template is checked before anything is sent.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

use crate::ids::{ClubId, ClubMailId};
use minijinja::{
    AutoEscape, Environment, UndefinedBehavior,
    value::{Serde, Value},
};
use time::OffsetDateTime;
use tracing::{error, warn};
use uuid::Uuid;

use crate::{
    act_type::ActType,
    bookings::{paid_by_club, round_cents},
    competition::{Competition, ResolvedFeatures},
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

/// The acts `club_id` looks after that have no music: its singles and pairs
/// and the groups it created, without withdrawn ones; by type, then name.
async fn acts_without_music(
    db: &sqlx::SqlitePool,
    club_id: ClubId,
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
        let id = ClubId(Uuid::nil());
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
        let ids = sqlx::query_scalar!(r#"SELECT id as "id!: ClubId" FROM clubs ORDER BY name"#)
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
        ids: &[ClubId],
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
                WHERE clubs.id = $1
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

/// What a variable needs besides the permission to send club mails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    Nothing,
    /// Money: the permission to see club balances.
    Balances,
    /// The jury obligation: the jury or the club balances, while the jury
    /// registration is on.
    Jury,
}

/// What the sender of a club mail may use of the clubs' data: the same rule
/// as the club list's balances and jury obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Access {
    /// Money: with the permission to see club balances.
    pub balances: bool,
    /// The jury obligation: with the jury or the club balances, while the
    /// jury registration is on.
    pub jury: bool,
    /// Whether the jury registration is on: then the amount due includes
    /// the jury fee.
    pub jury_registration: bool,
}

impl Access {
    /// What `auth` may use with the competition's `features`.
    pub fn of(auth: &Auth, features: ResolvedFeatures) -> Self {
        let balances = auth.can(Permission::ViewClubBalances);
        Self {
            balances,
            jury: features.jury_registration && (balances || auth.can(Permission::ManageJury)),
            jury_registration: features.jury_registration,
        }
    }

    /// Whether a variable that `needs` this may be used.
    pub fn allows(self, needs: Needs) -> bool {
        match needs {
            Needs::Nothing => true,
            Needs::Balances => self.balances,
            Needs::Jury => self.jury,
        }
    }
}

/// A variable of the mail templates. A new variable is one more entry in
/// [`VARIABLES`].
pub struct Variable {
    pub name: &'static str,
    /// What it holds, in German for the mail form.
    pub description: &'static str,
    pub needs: Needs,
    value: fn(&ClubData) -> Value,
}

/// Every variable the templates can use, in the order the mail form lists
/// them. Amounts are numbers in €; `{{ amount | euro }}` writes them like
/// "1.505,00 €".
pub const VARIABLES: &[Variable] = &[
    Variable {
        name: "club_name",
        description: "Name des Vereins",
        needs: Needs::Nothing,
        value: |club| Value::from(club.name.clone()),
    },
    Variable {
        name: "club_code",
        description: "Vereinscode für den Verwendungszweck, z. B. EV-K7Q3",
        needs: Needs::Nothing,
        value: |club| Value::from(club.code.clone()),
    },
    Variable {
        name: "contact_name",
        description: "Name der Kontaktperson des Vereins",
        needs: Needs::Nothing,
        value: |club| Value::from(club.contact_name.clone()),
    },
    Variable {
        name: "amount_due",
        description: "Betrag: Startgeld und Jurygebühr abzüglich Erlasse (Zahl in €)",
        needs: Needs::Balances,
        value: |club| Value::from(club.amount_due),
    },
    Variable {
        name: "paid",
        description: "Gezahlt (Zahl in €)",
        needs: Needs::Balances,
        value: |club| Value::from(club.paid),
    },
    Variable {
        name: "open_amount",
        description: "Offen: Betrag minus Gezahlt, negativ bei Überzahlung (Zahl in €)",
        needs: Needs::Balances,
        value: |club| Value::from(club.open_amount()),
    },
    Variable {
        name: "missing_deployments",
        description: "Fehlende Juryeinsätze",
        needs: Needs::Jury,
        value: |club| Value::from(club.missing_deployments),
    },
    Variable {
        name: "missing_music",
        description: "Küren ohne Musik, jede mit name (Gruppe oder FahrerInnen), type (z. B. Einzelkür) und title (Kürtitel)",
        needs: Needs::Nothing,
        value: |club| Value::from(Serde(&club.missing_music)),
    },
];

/// The variables of `club` that `access` allows, for a template.
fn context(club: &ClubData, access: Access) -> Value {
    let variables: BTreeMap<&str, Value> = VARIABLES
        .iter()
        .filter(|variable| access.allows(variable.needs))
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
    pub club_ids: Vec<ClubId>,
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
    pub club_id: Option<ClubId>,
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

/// A template that compiled, uses only variables its sender may use and
/// rendered for a sample club.
struct MailTemplate<'a> {
    subject: &'a str,
    body: &'a str,
    access: Access,
    /// The variables it uses, in the order of [`VARIABLES`].
    uses: Vec<&'static str>,
}

impl<'a> MailTemplate<'a> {
    /// Checks the template against what `access` allows; the message is
    /// German.
    fn check(subject: &'a str, body: &'a str, access: Access) -> Result<Self, String> {
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
                .filter(|v| !access.allows(v.needs) && undeclared.contains(v.name))
                .map(|v| v.name)
                .collect();
            if !forbidden.is_empty() {
                return Err(format!(
                    "Nicht verfügbar im {part}: {} (Beträge nur für Kasse und Admin, \
                     Juryeinsätze nur mit der Jury-Anmeldung).",
                    forbidden.join(", ")
                ));
            }
            uses.extend(undeclared);
        }
        let template = Self {
            subject,
            body,
            access,
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
            |club| context(club, self.access),
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
    /// Where answers go: the competition's contact email. Without one, every
    /// mail ends with [`NO_REPLY_NOTE`].
    pub reply_to: Option<String>,
    pub mails: Vec<RenderedMail>,
}

/// Ends every club mail without a Reply-To: answers would go to the sender
/// address, which nobody reads. The same note as the other mails'.
pub const NO_REPLY_NOTE: &str =
    "P.S.: Bitte nicht auf diese Mail antworten - das Postfach wird nicht gelesen.";

/// Checks `input` and renders every mail, before anything is sent: one per
/// club, then one per typed-in address. An address gets each mail once:
/// a club given twice, or a typed-in address that already gets that
/// club's mail, adds nothing.
pub async fn prepare(
    db: &sqlx::SqlitePool,
    input: &ClubMailInput,
    access: Access,
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
    let template = MailTemplate::check(&input.subject, &input.body, access).map_err(invalid)?;
    if let Some((email, _)) = extra.iter().find(|(_, club_id)| club_id.is_none())
        && !template.uses.is_empty()
    {
        return Err(invalid(format!(
            "{email} gehört zu keinem Verein, die Mail braucht aber Vereinsdaten ({}). \
             Ordne die Adresse einem Verein zu oder lass die Variablen weg.",
            template.uses.join(", ")
        )));
    }

    let mut club_ids: Vec<ClubId> = Vec::new();
    for id in input
        .club_ids
        .iter()
        .chain(extra.iter().filter_map(|(_, id)| id.as_ref()))
    {
        if !club_ids.contains(id) {
            club_ids.push(*id);
        }
    }
    let clubs: HashMap<ClubId, ClubData> = ClubData::load(db, &club_ids, access.jury_registration)
        .await?
        .ok_or(HttpError::NotFound)?
        .into_iter()
        .map(|club| (club.id, club))
        .collect();
    let reply_to = reply_to(db).await?;

    // Each mail once per address: (address, club whose data it has).
    let mut covered: HashSet<(String, Option<ClubId>)> = HashSet::new();
    let mut recipients: Vec<(Option<&ClubData>, Vec<String>)> = Vec::new();
    let mut seen_clubs = HashSet::new();
    for id in &input.club_ids {
        if !seen_clubs.insert(*id) {
            continue;
        }
        let club = &clubs[id];
        let to = club.recipients();
        for address in &to {
            if address.parse::<lettre::Address>().is_err() {
                return Err(invalid(format!(
                    "{} hat eine ungültige Adresse: {address}. Korrigiere sie oder wähle \
                     den Verein ab.",
                    club.name
                )));
            }
            covered.insert((address.to_lowercase(), Some(club.id)));
        }
        recipients.push((Some(club), to));
    }
    for (email, club_id) in &extra {
        if covered.insert((email.to_lowercase(), *club_id)) {
            recipients.push((club_id.map(|id| &clubs[&id]), vec![email.clone()]));
        }
    }
    let mails = recipients
        .into_iter()
        .map(|(club, to)| {
            let (subject, body) = template.render(club)?;
            let body = if reply_to.is_some() {
                body
            } else {
                format!("{}\n\n{NO_REPLY_NOTE}", body.trim_end())
            };
            Ok(RenderedMail {
                club_id: club.map(|club| club.id),
                club_name: club.map(|club| club.name.clone()),
                to,
                subject,
                body,
            })
        })
        .collect::<Result<_, String>>()
        .map_err(invalid)?;
    Ok(PreparedMails { reply_to, mails })
}

/// A club as the mail form lists it, to choose the recipients.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct MailClub {
    pub id: ClubId,
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

/// The mail form's clubs, and the variables `access` allows.
pub async fn options(db: &sqlx::SqlitePool, access: Access) -> sqlx::Result<ClubMailOptions> {
    let clubs = ClubData::load_all(db, access.jury_registration)
        .await?
        .into_iter()
        .map(|club| {
            let own = club.has_own_representative();
            MailClub {
                open_amount: access.balances.then(|| club.open_amount()),
                missing_deployments: access.jury.then_some(club.missing_deployments),
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
            .filter(|variable| access.allows(variable.needs))
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
    #[serde(skip)]
    id: i64,
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

/// Keeps `prepared`, a club mail `author` writes, before anything goes out:
/// every mail as pending. Returns the log entry, mails in the order of
/// `prepared`, for [`send`].
pub async fn record(
    db: &sqlx::SqlitePool,
    author: &Auth,
    input: &ClubMailInput,
    prepared: PreparedMails,
) -> sqlx::Result<SentClubMail> {
    let id = ClubMailId::new();
    let sent_at = OffsetDateTime::now_utc();
    let mut tx = begin_write(db).await?;
    sqlx::query!(
        r#"
        INSERT INTO club_mails (id, sent_at, user_id, user_name, subject, body, reply_to)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        id,
        sent_at,
        author.user_id,
        author.name,
        input.subject,
        input.body,
        prepared.reply_to,
    )
    .execute(&mut *tx)
    .await?;
    let mut mails = Vec::with_capacity(prepared.mails.len());
    for mail in prepared.mails {
        let addresses = serde_json::to_string(&mail.to).expect("addresses serialize");
        let mail_id = sqlx::query_scalar!(
            r#"
            INSERT INTO club_mail_messages (mail_id, club_id, club_name, addresses, subject, body)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id as "id!: i64"
            "#,
            id,
            mail.club_id,
            mail.club_name,
            addresses,
            mail.subject,
            mail.body,
        )
        .fetch_one(&mut *tx)
        .await?;
        mails.push(SentMail {
            id: mail_id,
            club_id: mail.club_id,
            club_name: mail.club_name,
            to: mail.to,
            subject: mail.subject,
            body: mail.body,
            status: DeliveryStatus::Pending,
            error: None,
        });
    }
    tx.commit().await?;
    Ok(SentClubMail {
        id,
        sent_at,
        sender_name: author.name.clone(),
        subject: input.subject.clone(),
        body: input.body.clone(),
        reply_to: prepared.reply_to,
        mails,
    })
}

/// Sends the mails of `club_mail` one by one and keeps whether each went
/// out. Nothing stops it halfway: a mail that fails, or a status the
/// database doesn't take, doesn't keep the others from going out. Owns its
/// arguments, so it can run on after the request is gone.
pub async fn send(
    db: sqlx::SqlitePool,
    mailer: Arc<Mailer>,
    mut club_mail: SentClubMail,
) -> SentClubMail {
    for mail in &mut club_mail.mails {
        let to: Vec<&str> = mail.to.iter().map(String::as_str).collect();
        let result = mailer
            .send(&TextMail {
                to: &to,
                reply_to: club_mail.reply_to.as_deref(),
                subject: &mail.subject,
                body: &mail.body,
            })
            .await;
        (mail.status, mail.error) = match result {
            Ok(()) => (DeliveryStatus::Sent, None),
            Err(error) => {
                warn!("Club mail {} to {to:?} failed: {error}", club_mail.id);
                (DeliveryStatus::Failed, Some(error.to_string()))
            }
        };
        if let Err(error) = sqlx::query!(
            "UPDATE club_mail_messages SET status = $1, error = $2 WHERE id = $3",
            mail.status,
            mail.error,
            mail.id,
        )
        .execute(&db)
        .await
        {
            error!(
                "Couldn't keep the status of club mail {} to {to:?}: {error}",
                club_mail.id
            );
        }
    }
    club_mail
}

/// Every club mail sent, newest first.
pub async fn log(db: &sqlx::SqlitePool) -> sqlx::Result<Vec<SentClubMail>> {
    let club_mails = sqlx::query!(
        r#"
        SELECT
            id as "id!: ClubMailId",
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
    let mails = sqlx::query!(
        r#"
        SELECT
            id as "id!: i64",
            mail_id as "mail_id!: ClubMailId",
            club_id as "club_id: ClubId",
            club_name,
            addresses,
            subject,
            body,
            status as "status!: DeliveryStatus",
            error
        FROM club_mail_messages
        ORDER BY id
        "#
    )
    .fetch_all(db)
    .await?;
    let mut by_club_mail: HashMap<ClubMailId, Vec<SentMail>> = HashMap::new();
    for row in mails {
        by_club_mail.entry(row.mail_id).or_default().push(SentMail {
            id: row.id,
            club_id: row.club_id,
            club_name: row.club_name,
            to: serde_json::from_str(&row.addresses).unwrap_or_else(|error| {
                warn!(
                    "Club mail {} has unreadable addresses: {error}",
                    row.mail_id
                );
                Vec::new()
            }),
            subject: row.subject,
            body: row.body,
            status: row.status,
            error: row.error,
        });
    }
    Ok(club_mails
        .into_iter()
        .map(|club_mail| SentClubMail {
            mails: by_club_mail.remove(&club_mail.id).unwrap_or_default(),
            id: club_mail.id,
            sent_at: club_mail.sent_at,
            sender_name: club_mail.user_name,
            subject: club_mail.subject,
            body: club_mail.body,
            reply_to: club_mail.reply_to,
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
