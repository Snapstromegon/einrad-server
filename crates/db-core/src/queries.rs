//! The read side of the DB contract: the [`Queries`] supertrait over one small
//! trait per API area (#238). Every API querie endpoint is exactly one
//! method of exactly one area trait; `tests/db_contract.rs` guards that.
//! A backend implements the area traits, in separate files; `Queries` follows.

pub mod act;
pub mod admin;
pub mod category;
pub mod check_in;
pub mod club;
pub mod competition;
pub mod finals;
pub mod group;
pub mod heat;
pub mod jury;
pub mod media;
pub mod payment;
pub mod timeplan;
pub mod user;

pub use act::ActQueries;
pub use admin::AdminQueries;
pub use category::CategoryQueries;
pub use check_in::CheckInQueries;
pub use club::ClubQueries;
pub use competition::CompetitionQueries;
pub use finals::FinalQueries;
pub use group::GroupQueries;
pub use heat::HeatQueries;
pub use jury::JuryQueries;
pub use media::MediaQueries;
pub use payment::PaymentQueries;
pub use timeplan::TimeplanQueries;
pub use user::UserQueries;

/// All queries of the API. Implemented for everything that implements every
/// area trait.
pub trait Queries:
    ClubQueries
    + GroupQueries
    + PaymentQueries
    + CheckInQueries
    + CategoryQueries
    + HeatQueries
    + FinalQueries
    + TimeplanQueries
    + JuryQueries
    + CompetitionQueries
    + MediaQueries
    + UserQueries
    + AdminQueries
    + ActQueries
{
}

impl<T> Queries for T where
    T: ?Sized
        + ClubQueries
        + GroupQueries
        + PaymentQueries
        + CheckInQueries
        + CategoryQueries
        + HeatQueries
        + FinalQueries
        + TimeplanQueries
        + JuryQueries
        + CompetitionQueries
        + MediaQueries
        + UserQueries
        + AdminQueries
        + ActQueries
{
}
