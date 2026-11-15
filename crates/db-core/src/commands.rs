//! The write side of the DB contract: the [`Commands`] supertrait over one small
//! trait per API area (#238). Every API command endpoint is exactly one
//! method of exactly one area trait; `tests/db_contract.rs` guards that.
//! A backend implements the area traits, in separate files; `Commands` follows.

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

pub use act::ActCommands;
pub use admin::AdminCommands;
pub use category::CategoryCommands;
pub use check_in::CheckInCommands;
pub use club::ClubCommands;
pub use competition::CompetitionCommands;
pub use finals::FinalCommands;
pub use group::GroupCommands;
pub use heat::HeatCommands;
pub use jury::JuryCommands;
pub use media::MediaCommands;
pub use payment::PaymentCommands;
pub use timeplan::TimeplanCommands;
pub use user::UserCommands;

/// All commands of the API. Implemented for everything that implements every
/// area trait.
pub trait Commands:
    ClubCommands
    + GroupCommands
    + PaymentCommands
    + CheckInCommands
    + CategoryCommands
    + HeatCommands
    + FinalCommands
    + TimeplanCommands
    + JuryCommands
    + CompetitionCommands
    + MediaCommands
    + UserCommands
    + AdminCommands
    + ActCommands
{
}

impl<T> Commands for T where
    T: ?Sized
        + ClubCommands
        + GroupCommands
        + PaymentCommands
        + CheckInCommands
        + CategoryCommands
        + HeatCommands
        + FinalCommands
        + TimeplanCommands
        + JuryCommands
        + CompetitionCommands
        + MediaCommands
        + UserCommands
        + AdminCommands
        + ActCommands
{
}
