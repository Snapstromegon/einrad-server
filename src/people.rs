//! A club registers each rider and each judge once: names count as the same
//! ignoring case and surrounding spaces. Clubs read "Max Müller ist bereits
//! gemeldet." instead of a database error.

use sqlx::SqliteConnection;
use uuid::Uuid;

use crate::http_server::HttpError;

/// A first or last name as stored: without surrounding spaces.
pub fn trim_name(name: &mut String) {
    *name = name.trim().to_string();
}

/// Compares people's names like the "once per club" rule does: ignoring case
/// and surrounding spaces.
pub fn person_key(firstname: &str, lastname: &str) -> String {
    format!("{} {}", firstname.trim(), lastname.trim()).to_lowercase()
}

/// The German message for a rider or judge the club already registered.
pub fn already_registered(firstname: &str, lastname: &str) -> HttpError {
    HttpError::ErrorMessages(format!("{firstname} {lastname} ist bereits gemeldet."))
}

/// Who a club registers once per name.
#[derive(Debug, Clone, Copy)]
pub enum Person {
    Rider,
    Judge,
}

/// Fails with [`already_registered`] if the club already registered a
/// `person` with this name, giving the name as registered (it may differ in
/// case). `except_id` excludes the rider or judge being edited.
pub async fn ensure_name_is_free(
    conn: &mut SqliteConnection,
    person: Person,
    club_id: Uuid,
    firstname: &str,
    lastname: &str,
    except_id: Option<Uuid>,
) -> Result<(), HttpError> {
    let names: Vec<(String, String)> = match person {
        Person::Rider => sqlx::query_as(
            "SELECT firstname, lastname FROM rider WHERE club_id = ? AND id IS NOT ?",
        ),
        Person::Judge => sqlx::query_as(
            "SELECT firstname, lastname FROM judge WHERE club_id = ? AND id IS NOT ?",
        ),
    }
    .bind(club_id)
    .bind(except_id)
    .fetch_all(&mut *conn)
    .await?;
    let key = person_key(firstname, lastname);
    match names
        .iter()
        .find(|(first, last)| person_key(first, last) == key)
    {
        Some((first, last)) => Err(already_registered(first, last)),
        None => Ok(()),
    }
}

/// The database's own uniqueness rule, for the rare request that slips past
/// [`ensure_name_is_free`] at the same moment: the same message.
pub fn map_duplicate(error: sqlx::Error, firstname: &str, lastname: &str) -> HttpError {
    if is_unique_violation(&error) {
        already_registered(firstname, lastname)
    } else {
        error.into()
    }
}

pub(crate) fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|e| e.is_unique_violation())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_ignoring_case_and_spaces() {
        assert_eq!(person_key(" Max ", "MÜLLER"), person_key("max", "müller"));
        assert_ne!(person_key("Max", "Muller"), person_key("Max", "Müller"));
    }
}
