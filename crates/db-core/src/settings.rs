//! Competition settings: the [`SettingsRepo`] trait.

use crate::DbError;

#[async_trait::async_trait]
pub trait SettingsRepo {
    /// The name mails and pages use for the event: the current competition's
    /// name, or a neutral fallback before one is configured.
    async fn event_name(&self) -> Result<String, DbError>;
}
