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

use crate::ids::ClubId;
use db_core::{Db, DbError, DbHandle};
use minijinja::{
    AutoEscape, Environment, UndefinedBehavior,
    value::{Serde, Value},
};
use tracing::{error, warn};
use uuid::Uuid;

use crate::{
    act_type::ActType,
    competition::ResolvedFeatures,
    http_server::{HttpError, extractor::auth::Auth},
    mailer::{Mailer, TextMail},
    roles::Permission,
    utils::club_code,
};

pub use db_core::club_mails::{
    ActWithoutMusic, ClubData, DeliveryStatus, RenderedMail, SentClubMail, SentMail,
};

/// A made-up club, to try a template before sending it.
fn sample_club() -> ClubData {
    let id = ClubId(Uuid::nil());
    ClubData {
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
        template.render(Some(&sample_club()))?;
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
    repo: &dyn Db,
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
    let clubs: HashMap<ClubId, ClubData> = repo
        .load_mail_clubs(&club_ids, access.jury_registration)
        .await?
        .ok_or(HttpError::NotFound)?
        .into_iter()
        .map(|club| (club.id, club))
        .collect();
    let reply_to = repo.mail_reply_to().await?;

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
pub async fn options(repo: &dyn Db, access: Access) -> Result<ClubMailOptions, DbError> {
    let clubs = repo
        .load_all_mail_clubs(access.jury_registration)
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
        reply_to: repo.mail_reply_to().await?,
    })
}

/// Sends the mails of `club_mail` one by one and keeps whether each went
/// out. Nothing stops it halfway: a mail that fails, or a status the
/// database doesn't take, doesn't keep the others from going out. Owns its
/// arguments, so it can run on after the request is gone.
pub async fn send(
    repo: DbHandle,
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
        if let Err(error) = repo
            .set_mail_status(mail.id, mail.status, mail.error.as_deref())
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
