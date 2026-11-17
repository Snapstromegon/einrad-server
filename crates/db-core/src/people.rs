//! A club registers each rider and each judge once: names count as the same
//! ignoring case and surrounding spaces.

/// A first or last name as stored: without surrounding spaces.
pub fn trim_name(name: &mut String) {
    *name = name.trim().to_string();
}

/// Compares people's names like the "once per club" rule does: ignoring case
/// and surrounding spaces.
#[must_use]
pub fn person_key(firstname: &str, lastname: &str) -> String {
    format!("{} {}", firstname.trim(), lastname.trim()).to_lowercase()
}

/// The German message for a rider or judge the club already registered.
#[must_use]
pub fn already_registered_message(firstname: &str, lastname: &str) -> String {
    format!("{firstname} {lastname} ist bereits gemeldet.")
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
