//! Keeps secrets (passwords, tokens, keys) out of `Debug` output, and so out
//! of logs and the span fields `#[tracing::instrument]` records.

/// Debug-prints a secret as `***`. Use it in hand-written `Debug` impls.
/// An unset optional secret still shows as `None`.
pub struct Redacted<'a, T: ?Sized>(pub &'a T);

macro_rules! redacted_always {
    ($($ty:ty),*) => {$(
        impl std::fmt::Debug for Redacted<'_, $ty> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("***")
            }
        }
    )*};
}
redacted_always!(String, str, uuid::Uuid);

impl std::fmt::Debug for Redacted<'_, Option<String>> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(_) => f.write_str("Some(***)"),
            None => f.write_str("None"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Redacted;

    #[test]
    fn secrets_print_as_stars() {
        assert_eq!(format!("{:?}", Redacted(&"geheim".to_string())), "***");
        assert_eq!(
            format!("{:?}", Redacted(&Some("geheim".to_string()))),
            "Some(***)"
        );
        assert_eq!(format!("{:?}", Redacted(&None::<String>)), "None");
    }
}
