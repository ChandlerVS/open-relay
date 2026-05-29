//! User domain logic that's reachable from HTTP handlers.
//!
//! Argon2 hashing and SeaORM persistence live here rather than in
//! `crates/core` because they're infrastructure-coupled. If a non-HTTP
//! caller (CLI seed command, worker) ever needs user creation, lift
//! `service.rs` into `crates/core/src/users/` — the API stays the same.

pub mod dto;
pub mod service;
