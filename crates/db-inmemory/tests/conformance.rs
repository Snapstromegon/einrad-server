//! Runs the backend conformance suite against a fresh in-memory database.

db_core::conformance_tests!(|| async { db_inmemory::handle().await });
