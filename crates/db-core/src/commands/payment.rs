//! Payment commands: bookings and fee waivers.
//!
//! The request types are DB-facing: the handler converts its API body into
//! them. A booking's input is checked ([`BookingInput::checked`]) before it
//! gets here.

use crate::{
    actor::Actor, bookings::BookingInput, error::CommandError, fee_waivers::Target, ids::BookingId,
};

/// Records a booking and its log entry.
#[derive(Debug, Clone)]
pub struct CreateBooking {
    /// Already [`BookingInput::checked`].
    pub input: BookingInput,
}

/// Changes a booking; logs only if something changed.
#[derive(Debug, Clone)]
pub struct EditBooking {
    pub booking_id: BookingId,
    /// Already [`BookingInput::checked`].
    pub input: BookingInput,
}

/// Deletes a booking; the log keeps it.
#[derive(Debug, Clone)]
pub struct DeleteBooking {
    pub booking_id: BookingId,
}

/// Waives the fees of `target` with `reason`, or charges them again with
/// `None`. A blank reason is refused (`Rejected`), a reason is trimmed.
#[derive(Debug, Clone)]
pub struct SetFeeWaived {
    pub target: Target,
    pub reason: Option<String>,
}

/// The write side of the payment area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait PaymentCommands: Send + Sync {
    /// `NotFound` if the booking's club doesn't exist.
    async fn create_booking(
        &self,
        actor: &Actor,
        cmd: CreateBooking,
    ) -> Result<BookingId, CommandError>;

    /// `NotFound` for an unknown booking or club.
    async fn edit_booking(&self, actor: &Actor, cmd: EditBooking) -> Result<(), CommandError>;

    /// `NotFound` for an unknown booking.
    async fn delete_booking(&self, actor: &Actor, cmd: DeleteBooking) -> Result<(), CommandError>;

    /// Writes a log entry only if that changes anything. `NotFound` for an
    /// unknown target, `Rejected` for a blank reason.
    async fn set_fee_waived(&self, actor: &Actor, cmd: SetFeeWaived) -> Result<(), CommandError>;
}
