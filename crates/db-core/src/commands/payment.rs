//! Payment commands: bookings and fee waivers.

/// The write side of the payment area of the API; one method per endpoint.
pub trait PaymentCommands: Send + Sync {}
