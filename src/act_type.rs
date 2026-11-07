use serde::{Deserialize, Serialize};

/// Groups need at least this many regular riders (replacements don't count).
/// Smaller groups stay registered, but get no category (see `view_act`).
pub const MIN_GROUP_SIZE: i64 = 3;

/// A small group (Kleingruppe) has up to this many regular riders, a large
/// group (Großgruppe) more. The club chooses the type; a group outside its
/// type's range keeps its category, and the frontend warns about it.
pub const MAX_SMALL_GROUP_SIZE: i64 = 8;

/// The kind of an act. Stored as text in `acts.act_type` and `categories.act_type`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum ActType {
    Single,
    Pair,
    GroupSmall,
    GroupLarge,
}

impl ActType {
    pub fn as_str(self) -> &'static str {
        match self {
            ActType::Single => "single",
            ActType::Pair => "pair",
            ActType::GroupSmall => "group_small",
            ActType::GroupLarge => "group_large",
        }
    }

    /// The German name shown to users.
    pub fn german_name(self) -> &'static str {
        match self {
            ActType::Single => "Einzelkür",
            ActType::Pair => "Paarkür",
            ActType::GroupSmall => "Kleingruppe",
            ActType::GroupLarge => "Großgruppe",
        }
    }

    pub fn is_group(self) -> bool {
        matches!(self, ActType::GroupSmall | ActType::GroupLarge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [ActType; 4] = [
        ActType::Single,
        ActType::Pair,
        ActType::GroupSmall,
        ActType::GroupLarge,
    ];

    #[test]
    fn serializes_like_the_database_value() {
        for act_type in ALL {
            assert_eq!(
                serde_json::to_string(&act_type).unwrap(),
                format!("\"{}\"", act_type.as_str())
            );
        }
    }

    #[test]
    fn only_group_types_are_groups() {
        let groups: Vec<_> = ALL.into_iter().filter(|t| t.is_group()).collect();
        assert_eq!(groups, [ActType::GroupSmall, ActType::GroupLarge]);
    }
}
