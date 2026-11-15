use crate::ids::ClubId;

/// For `#[serde(default = "crate::utils::default_true")]`.
pub fn default_true() -> bool {
    true
}

/// Letters and digits that can't be confused with each other (no 0/O, 1/I).
const CLUB_CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// A short code for a club's transfer reference, like "EV-K7Q3".
///
/// It comes from the random end of the club's id, so it stays the same when
/// the club is renamed and needs no storage. Four characters give about a
/// million codes; with a few dozen clubs a clash is very unlikely, and the
/// club name is part of the reference anyway.
///
/// ```
/// use einrad_server::utils::club_code;
/// use einrad_server::ids::ClubId;
///
/// let id: ClubId = "0199d4a2-7c3e-7000-8000-00000000abcd".parse().unwrap();
/// assert_eq!(club_code(&id), club_code(&id));
/// assert!(club_code(&id).starts_with("EV-"));
/// assert_eq!(club_code(&id).len(), 7);
/// ```
pub fn club_code(id: &ClubId) -> String {
    let bytes = id.0.as_bytes();
    let bits = u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    let code: String = (0..4)
        .map(|i| CLUB_CODE_ALPHABET[((bits >> (5 * i)) & 31) as usize] as char)
        .collect();
    format!("EV-{code}")
}

/// Checks that a password satisfies the registration password policy.
///
/// A password must be at least 8 bytes long and contain characters from at
/// least three of these categories: lowercase letters, uppercase letters,
/// digits and special characters. The returned error is a user-facing
/// (German) message.
///
/// ```
/// use einrad_server::utils::check_password;
///
/// assert!(check_password("Secret123").is_ok());
/// assert!(check_password("geheim-123").is_ok());
///
/// // Too short
/// assert!(check_password("Ab1!").is_err());
/// // Only two categories (lowercase and digits)
/// assert!(check_password("password123").is_err());
/// ```
pub fn check_password(password: &str) -> Result<(), String> {
    if password.len() < 8 {
        return Err("Passwort muss mindestens 8 Zeichen haben.".to_string());
    }

    let mut categories = 0;
    if password.chars().any(|c| c.is_uppercase()) {
        categories += 1;
    }
    if password.chars().any(|c| c.is_lowercase()) {
        categories += 1;
    }
    if password.chars().any(|c| c.is_numeric()) {
        categories += 1;
    }
    if r#"!"§$%&/()=?+-*#'_~.,:;<>|\\ {[]}"#
        .chars()
        .any(|s| password.chars().any(|c| s == c))
    {
        categories += 1;
    }

    if categories < 3 {
        return Err("Passwort muss mindestens 3 der folgenden Kategorien enthalten: Kleinbuchstaben, Großbuchstaben, Zahlen, Sonderzeichen.".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_shorter_than_eight_is_rejected() {
        let err = check_password("Ab1!xyz").unwrap_err();
        assert!(err.contains("mindestens 8 Zeichen"), "{err}");
    }

    #[test]
    fn password_with_exactly_eight_chars_and_three_categories_is_accepted() {
        assert_eq!(check_password("Abcdefg1"), Ok(()));
    }

    #[test]
    fn password_with_two_categories_is_rejected() {
        for password in [
            "abcdefgh1",
            "ABCDEFGH1",
            "abcdEFGH",
            "abcd!efgh",
            "1234!5678",
        ] {
            let err = check_password(password).unwrap_err();
            assert!(
                err.contains("3 der folgenden Kategorien"),
                "{password}: {err}"
            );
        }
    }

    #[test]
    fn password_with_three_or_four_categories_is_accepted() {
        for password in [
            "abcdEFGH1",
            "abcd!efg1",
            "ABCD!EFG1",
            "abcd!EFGH",
            "aB1!aB1!",
        ] {
            assert_eq!(check_password(password), Ok(()), "{password}");
        }
    }

    #[test]
    fn space_and_backslash_count_as_special_characters() {
        assert_eq!(check_password("abcd efg1"), Ok(()));
        assert_eq!(check_password("abcd\\efg1"), Ok(()));
    }

    #[test]
    fn non_ascii_letters_count_as_letters() {
        // "Ä" is uppercase, "ö" lowercase.
        assert_eq!(check_password("Äöxxxxx1"), Ok(()));
    }

    #[test]
    fn length_check_happens_before_category_check() {
        let err = check_password("abc").unwrap_err();
        assert!(err.contains("mindestens 8 Zeichen"), "{err}");
    }
}
