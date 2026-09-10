//! Shared handling for secret-bearing keys inside a JSON config blob.
//!
//! Two resources now store credentials the same way — a `kind`-discriminated
//! JSON `config` column whose secret-bearing top-level keys are declared by the
//! matching factory (`BackendFactory::secret_keys`,
//! `FileStoreFactory::secret_keys`) and AEAD-encrypted in place. The functions
//! here are that machinery, taking the declared key list directly rather than a
//! registry, so one copy serves both.
//!
//! The ordering these enable is the subtle part, and both callers follow it on
//! update:
//!
//! ```text
//! preserve   — carry over secrets the client omitted (it never received them)
//! decrypt    — bring every secret to a uniform plaintext view
//! validate   — run the factory against plaintext, so it can reject a bad token
//! encrypt    — re-seal before the row is written
//! ```
//!
//! Getting it wrong in either direction is silent: skip `preserve` and a
//! round-tripped admin form wipes the credential; skip `decrypt` and the
//! factory validates ciphertext and stores a double-encrypted value.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::crypto::SecretCipher;
use crate::error::CoreResult;

/// A JSON value counts as "empty" (no secret on record) when it's null or an
/// empty/whitespace-only string.
pub fn value_is_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        _ => false,
    }
}

/// For each declared secret key, if `incoming` omits it (absent, null, or
/// empty string) but `existing` has a value, carry the existing value over.
///
/// This is what makes "leave the field blank to keep the current secret" work:
/// the admin DTO strips secrets, so a config the client read back and re-sent
/// necessarily omits them.
pub fn preserve(secret_keys: &[&'static str], existing: &Value, incoming: &mut Value) {
    let Some(incoming_obj) = incoming.as_object_mut() else {
        return;
    };
    for key in secret_keys {
        let incoming_empty = incoming_obj.get(*key).map(value_is_empty).unwrap_or(true);
        if !incoming_empty {
            continue;
        }
        if let Some(existing_val) = existing.get(*key).filter(|v| !value_is_empty(v)) {
            incoming_obj.insert((*key).to_string(), existing_val.clone());
        }
    }
}

/// Encrypt each declared secret key holding a non-empty string that isn't
/// already ciphertext. Idempotent, thanks to the `enc:v1:` prefix check — safe
/// to call on a config whose secrets are already sealed.
pub fn encrypt_in_place(
    secret_keys: &[&'static str],
    config: &mut Value,
    cipher: &SecretCipher,
) -> CoreResult<()> {
    let Some(obj) = config.as_object_mut() else {
        return Ok(());
    };
    for key in secret_keys {
        if let Some(Value::String(s)) = obj.get(*key)
            && !s.trim().is_empty()
            && !SecretCipher::is_encrypted(s)
        {
            let enc = cipher.encrypt(s)?;
            obj.insert((*key).to_string(), Value::String(enc));
        }
    }
    Ok(())
}

/// Decrypt each declared secret key back to plaintext. A legacy plaintext value
/// (no `enc:v1:` prefix) passes through unchanged.
pub fn decrypt_in_place(
    secret_keys: &[&'static str],
    config: &mut Value,
    cipher: &SecretCipher,
) -> CoreResult<()> {
    let Some(obj) = config.as_object_mut() else {
        return Ok(());
    };
    for key in secret_keys {
        if let Some(Value::String(s)) = obj.get(*key) {
            let plain = cipher.decrypt(s)?;
            obj.insert((*key).to_string(), Value::String(plain));
        }
    }
    Ok(())
}

/// Strip every declared secret key out of `config` and report which of them had
/// a value. The pair is what an admin-facing DTO exposes: never the secret, but
/// enough for the UI to render "set" vs "not set".
pub fn redact(secret_keys: &[&'static str], config: &mut Value) -> BTreeMap<String, bool> {
    let mut present = BTreeMap::new();
    for key in secret_keys {
        let has = config.get(*key).map(|v| !value_is_empty(v)).unwrap_or(false);
        present.insert((*key).to_string(), has);
        if let Some(obj) = config.as_object_mut() {
            obj.remove(*key);
        }
    }
    present
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const KEYS: &[&str] = &["token"];

    fn cipher() -> SecretCipher {
        SecretCipher::from_key_bytes(&[5u8; crate::crypto::KEY_LEN]).unwrap()
    }

    #[test]
    fn preserve_carries_over_an_omitted_secret() {
        let existing = json!({ "token": "enc:v1:abc", "id": "old" });
        let mut incoming = json!({ "id": "new" });
        preserve(KEYS, &existing, &mut incoming);
        assert_eq!(incoming["token"], "enc:v1:abc");
        assert_eq!(incoming["id"], "new");
    }

    #[test]
    fn preserve_treats_empty_string_as_omitted() {
        let existing = json!({ "token": "enc:v1:abc" });
        let mut incoming = json!({ "token": "   " });
        preserve(KEYS, &existing, &mut incoming);
        assert_eq!(incoming["token"], "enc:v1:abc");
    }

    #[test]
    fn preserve_does_not_clobber_a_supplied_secret() {
        let existing = json!({ "token": "enc:v1:abc" });
        let mut incoming = json!({ "token": "brand-new" });
        preserve(KEYS, &existing, &mut incoming);
        assert_eq!(incoming["token"], "brand-new");
    }

    #[test]
    fn encrypt_then_decrypt_round_trips_and_is_idempotent() {
        let c = cipher();
        let mut config = json!({ "token": "plain", "id": "x" });
        encrypt_in_place(KEYS, &mut config, &c).unwrap();
        let once = config["token"].as_str().unwrap().to_string();
        assert!(SecretCipher::is_encrypted(&once));
        // Second pass must not double-encrypt.
        encrypt_in_place(KEYS, &mut config, &c).unwrap();
        assert_eq!(config["token"].as_str().unwrap(), once);
        decrypt_in_place(KEYS, &mut config, &c).unwrap();
        assert_eq!(config["token"], "plain");
        assert_eq!(config["id"], "x");
    }

    #[test]
    fn redact_strips_and_reports_presence() {
        let mut config = json!({ "token": "sekrit", "id": "x" });
        let present = redact(KEYS, &mut config);
        assert!(config.get("token").is_none());
        assert_eq!(config["id"], "x");
        assert_eq!(present.get("token"), Some(&true));
    }

    #[test]
    fn redact_reports_absent_secret_as_not_set() {
        let mut config = json!({ "id": "x" });
        assert_eq!(redact(KEYS, &mut config).get("token"), Some(&false));
    }
}
