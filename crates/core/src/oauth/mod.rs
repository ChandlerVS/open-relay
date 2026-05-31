//! OAuth/OIDC flow primitives.
//!
//! - `discovery` fetches and parses an OIDC `.well-known/openid-configuration`.
//! - `idtoken` validates the signed `id_token` (JWKS signature + claims).
//! - `oidc` is a generic `Provider` implementation driven by stored config.
//! - `state` issues and verifies the HMAC-signed CSRF/PKCE envelope cookie
//!   that round-trips during the authorize → callback redirect.

pub mod discovery;
pub mod idtoken;
pub mod oidc;
pub mod state;
