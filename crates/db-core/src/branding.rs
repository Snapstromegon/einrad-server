//! The competition file's branding settings.

use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// A branding setting of the competition file, like the logo (ADR 0009): a
/// replace keeps what the database has unless the file sets it. Left out,
/// it's kept; an empty string resets it to the default built into the
/// binary; any other value sets it, checked by `T`'s `FromStr`.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Branding<T> {
    #[default]
    Keep,
    Reset,
    Set(T),
}

impl<T> Branding<T> {
    /// A setting as the admin UI saves it: `None` is the default.
    pub fn set_or_reset(value: Option<T>) -> Self {
        value.map_or(Branding::Reset, Branding::Set)
    }

    pub fn is_keep(&self) -> bool {
        matches!(self, Branding::Keep)
    }

    /// The value it sets; `None` if it keeps or resets it.
    pub fn value(&self) -> Option<&T> {
        match self {
            Branding::Set(value) => Some(value),
            Branding::Keep | Branding::Reset => None,
        }
    }
}

impl<'de, T> Deserialize<'de> for Branding<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.is_empty() {
            return Ok(Branding::Reset);
        }
        value.parse().map(Branding::Set).map_err(D::Error::custom)
    }
}

impl<T: std::fmt::Display> Serialize for Branding<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            // Left out of the file by `skip_serializing_if`.
            Branding::Keep => serializer.serialize_none(),
            Branding::Reset => serializer.serialize_str(""),
            Branding::Set(value) => serializer.serialize_str(&value.to_string()),
        }
    }
}
