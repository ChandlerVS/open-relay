//! OpenRelay core domain logic.
//!
//! Framework-agnostic: nothing in here knows about Axum, HTTP, or any
//! particular web stack. Holds the `Backend` delivery trait + registry, the
//! submission-delivery worker loop, user persistence + password hashing, the
//! auth primitives (JWT key material, claims, `Provider` trait for SSO), and
//! the wire-contract DTOs that describe what crosses the API boundary.
//!
//! HTTP wiring — error → response mapping, extractors, routers, the OpenAPI
//! aggregator — belongs in the server crate. `serde` and `utoipa` derives are
//! pure metadata; they don't pull a framework in.

pub mod auth;
pub mod backend;
pub mod error;
pub mod external_identity;
pub mod forms;
pub mod jobs;
pub mod oauth;
pub mod oauth_config;
pub mod permissions;
pub mod rbac;
pub mod setup;
pub mod users;
