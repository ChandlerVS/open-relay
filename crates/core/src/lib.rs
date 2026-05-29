//! OpenRelay core domain logic.
//!
//! Holds framework-agnostic pieces that don't depend on Axum: the `Backend`
//! delivery trait, its registry, and the submission-delivery worker loop.
//! Anything that touches HTTP belongs in `apps/server`.

pub mod backend;
pub mod jobs;
