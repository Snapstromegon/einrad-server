//! Typed entity IDs. Every entity key is a UUIDv7; a newtype per entity keeps
//! a heat ID from being passed where a final ID is expected.
//!
//! The newtypes are transparent for serde and sqlx, and render in the OpenAPI
//! schema as a plain `string`/`uuid`.
//!
//! There is deliberately no `Default` (it would mint a random ID) and no
//! `From<Uuid>`; build one explicitly with `XId(uuid)` or [`XId::new`].
//!
//! Note that `query!` binds accept any transparent newtype, so these types
//! protect function signatures and struct fields, not SQL parameters: binding
//! a `HeatId` where a final's ID belongs still compiles.

use uuid::Uuid;

macro_rules! typed_ids {
    ($($(#[$meta:meta])* $name:ident),+ $(,)?) => {$(
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
            serde::Serialize, serde::Deserialize, sqlx::Type,
        )]
        #[serde(transparent)]
        #[sqlx(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            /// A fresh time-ordered (v7) ID.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// The underlying UUID.
            #[must_use]
            pub fn uuid(self) -> Uuid {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse().map(Self)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl utoipa::PartialSchema for $name {
            fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
                utoipa::openapi::ObjectBuilder::new()
                    .schema_type(utoipa::openapi::schema::Type::String)
                    .format(Some(utoipa::openapi::schema::SchemaFormat::KnownFormat(
                        utoipa::openapi::schema::KnownFormat::Uuid,
                    )))
                    .into()
            }
        }

        impl utoipa::ToSchema for $name {}
    )+};
}

typed_ids! {
    ActId, BookingId, CategoryId, ClubId, ClubMailId, FinalId, FinalSlotId, HeatId, JudgeId,
    RiderId, UserId,
    /// The ID of a jury unit: the ID of a heat (its Vorlauf) or of a final.
    JuryUnitId,
    /// A connected music player (not stored in the database).
    PlayerId,
    /// An entry of the award ceremony queue (not stored in the database).
    QueueEntryId,
}

impl From<HeatId> for JuryUnitId {
    fn from(id: HeatId) -> Self {
        Self(id.0)
    }
}

impl From<FinalId> for JuryUnitId {
    fn from(id: FinalId) -> Self {
        Self(id.0)
    }
}

impl JuryUnitId {
    /// This unit as the heat it is the Vorlauf of.
    #[must_use]
    pub fn as_heat(self) -> HeatId {
        HeatId(self.0)
    }

    /// This unit as the final it is.
    #[must_use]
    pub fn as_final(self) -> FinalId {
        FinalId(self.0)
    }
}
