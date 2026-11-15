//! Payment queries: bookings and fee waivers.

/// The read side of the payment area of the API; one method per endpoint.
pub trait PaymentQueries: Send + Sync {}
