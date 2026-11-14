//! Competition settings: the [`SettingsRepo`] trait.

use crate::DbError;

/// The raw feature switches of the current competition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FeatureFlags {
    pub jury_registration: Option<bool>,
    pub moderation_input: Option<bool>,
}

#[async_trait::async_trait]
pub trait SettingsRepo {
    /// The features the current competition turns on or off; `None` is unset
    /// (the build default applies, see the application's `Features`).
    async fn feature_flags(&self) -> Result<FeatureFlags, DbError>;

    /// The name mails and pages use for the event: the current competition's
    /// name, or a neutral fallback before one is configured.
    async fn event_name(&self) -> Result<String, DbError>;
}
