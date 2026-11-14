//! Acts as the structure and the startlist read them.

use crate::ids::{ClubId, RiderId};

/// A participant of an act, as stored in the `participants` JSON of `view_act`.
#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema, Clone)]
pub struct ActParticipant {
    pub firstname: String,
    pub lastname: String,
    pub id: RiderId,
    pub club_id: ClubId,
    pub club_name: String,
    /// Replacement riders (Ersatzfahrer) of a group. Always false for singles
    /// and pairs.
    pub is_replacement: bool,
}
