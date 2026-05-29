//! OAuth/OIDC flow primitives.
//!
//! - `discovery` fetches and parses an OIDC `.well-known/openid-configuration`.
//! - `oidc` is a generic `Provider` implementation driven by stored config.
//! - `state` issues and verifies the HMAC-signed CSRF/PKCE envelope cookie
//!   that round-trips during the authorize → callback redirect.

pub mod discovery;
pub mod oidc;
pub mod state;
