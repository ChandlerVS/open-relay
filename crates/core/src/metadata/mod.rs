//! Dynamic per-form metadata — a typed Entity-Attribute-Value (EAV) framework.
//!
//! Lets us attach evolving options to a form without a schema migration per
//! feature. Storage is the `form_metadata` table (one `(form_id, key, value)`
//! row per attribute); this module is the typed layer over it.
//!
//! [`MetadataKey`] is the single source of truth for which keys exist — the
//! same enum-as-catalogue shape as [`crate::permissions::Permission`]. Each key
//! declares a [`MetadataValueType`], and the service layer type-checks values
//! against it on write. The DB only ever sees a string (see
//! [`MetadataValue::to_storage`]).
//!
//! To add a key: add a variant with its `#[serde(rename = "...")]` slug, wire
//! it into `slug`/`from_slug`/`value_type`. Adding a *value type* (int, text,
//! …) is a code-only change to [`MetadataValueType`]/[`MetadataValue`].
//!
//! `from_slug` is `Option` on purpose: an unknown slug read back from the DB (a
//! row written by a since-removed variant) is silently dropped, not surfaced as
//! a 500 — mirroring `Permission::from_slug`.

pub mod service;

use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use utoipa::ToSchema;

use crate::error::{CoreError, CoreResult};

/// The catalogue of metadata keys. Wire/serialized form is the slug.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, strum::EnumIter,
)]
pub enum MetadataKey {
    /// Boolean toggle: reject a submission whose email already exists for the
    /// form. Key + plumbing only — the dedup check itself is not implemented.
    #[serde(rename = "email_deduplication")]
    EmailDeduplication,
}

impl MetadataKey {
    pub fn all() -> Vec<Self> {
        <Self as IntoEnumIterator>::iter().collect()
    }

    pub fn slug(&self) -> &'static str {
        match self {
            Self::EmailDeduplication => "email_deduplication",
        }
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        match s {
            "email_deduplication" => Some(Self::EmailDeduplication),
            _ => None,
        }
    }

    /// The value type this key accepts. Writes are validated against it.
    pub fn value_type(&self) -> MetadataValueType {
        match self {
            Self::EmailDeduplication => MetadataValueType::Bool,
        }
    }
}

/// The kinds of value a [`MetadataKey`] can hold. Extend alongside
/// [`MetadataValue`] when a new key needs a non-boolean payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MetadataValueType {
    Bool,
}

/// A typed metadata value. The variant must match its key's
/// [`MetadataKey::value_type`]; the service layer enforces this on write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum MetadataValue {
    Bool(bool),
}

impl MetadataValue {
    pub fn value_type(&self) -> MetadataValueType {
        match self {
            Self::Bool(_) => MetadataValueType::Bool,
        }
    }

    /// Encode for the `form_metadata.value` column.
    pub fn to_storage(&self) -> String {
        match self {
            Self::Bool(b) => b.to_string(),
        }
    }

    /// Decode a stored value given the key's declared type. A row that fails to
    /// parse is corrupt for that type — surfaced as an internal error rather
    /// than silently coerced.
    pub fn from_storage(ty: MetadataValueType, raw: &str) -> CoreResult<Self> {
        match ty {
            MetadataValueType::Bool => raw
                .parse::<bool>()
                .map(Self::Bool)
                .map_err(|_| CoreError::Internal(anyhow::anyhow!("invalid bool metadata value"))),
        }
    }

    /// Convenience accessor for boolean keys.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
        }
    }
}

/// A key/value pair for a form, used by list results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MetadataEntry {
    pub key: MetadataKey,
    pub value: MetadataValue,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_keys_round_trip_through_slug() {
        let all = MetadataKey::all();
        assert!(!all.is_empty());
        for k in &all {
            assert_eq!(MetadataKey::from_slug(k.slug()), Some(*k));
        }
    }

    #[test]
    fn all_slugs_are_unique() {
        let slugs: HashSet<&'static str> = MetadataKey::all().iter().map(|k| k.slug()).collect();
        assert_eq!(slugs.len(), MetadataKey::all().len());
    }

    #[test]
    fn unknown_slug_returns_none() {
        assert!(MetadataKey::from_slug("nope").is_none());
    }

    #[test]
    fn bool_value_round_trips_through_storage() {
        for b in [true, false] {
            let v = MetadataValue::Bool(b);
            let decoded =
                MetadataValue::from_storage(MetadataValueType::Bool, &v.to_storage()).unwrap();
            assert_eq!(decoded, v);
            assert_eq!(decoded.as_bool(), Some(b));
        }
    }

    #[test]
    fn bad_bool_storage_fails_to_decode() {
        assert!(MetadataValue::from_storage(MetadataValueType::Bool, "notabool").is_err());
    }

    #[test]
    fn value_type_matches_key() {
        assert_eq!(
            MetadataKey::EmailDeduplication.value_type(),
            MetadataValue::Bool(true).value_type()
        );
    }
}
