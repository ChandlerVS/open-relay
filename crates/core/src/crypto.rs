//! AEAD encryption for secret columns at rest.
//!
//! Framework-agnostic, mirroring [`crate::auth::AuthKeys`]: the server builds a
//! [`SecretCipher`] from the validated `ENCRYPTION_KEY` and threads `&SecretCipher`
//! into the services that read/write secret-bearing columns
//! (`oauth_provider_config.client_secret`, backend-instance config tokens).
//!
//! Ciphertext is stored as a self-describing string `enc:v1:<base64(nonce||ct)>`.
//! [`SecretCipher::decrypt`] returns any input *without* the `enc:v1:` prefix
//! verbatim, so a value written before encryption was introduced (legacy
//! plaintext) round-trips untouched. New writes always go through [`SecretCipher::encrypt`].

use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand::RngCore;
use zeroize::Zeroizing;

use crate::error::{CoreError, CoreResult};

/// Required raw key length in bytes (XChaCha20-Poly1305 uses a 256-bit key).
pub const KEY_LEN: usize = 32;

/// XChaCha20 uses a 192-bit (24-byte) nonce, large enough to pick at random per
/// message without practical collision risk.
const NONCE_LEN: usize = 24;

/// Versioned prefix marking a value as ciphertext produced by this module.
const PREFIX: &str = "enc:v1:";

/// AEAD cipher for secret-at-rest columns. Holds the raw key zeroized; the
/// per-message cipher is cheap to construct so we don't keep it resident.
#[derive(Clone)]
pub struct SecretCipher {
    key: Zeroizing<[u8; KEY_LEN]>,
}

impl SecretCipher {
    /// Build from raw key bytes. Errors unless exactly [`KEY_LEN`] bytes.
    pub fn from_key_bytes(bytes: &[u8]) -> CoreResult<Self> {
        if bytes.len() != KEY_LEN {
            return Err(CoreError::BadRequest(format!(
                "encryption key must be exactly {KEY_LEN} bytes, got {}",
                bytes.len()
            )));
        }
        let mut key = [0u8; KEY_LEN];
        key.copy_from_slice(bytes);
        Ok(Self {
            key: Zeroizing::new(key),
        })
    }

    /// Build from a base64-encoded key (standard alphabet, padded or not).
    pub fn from_base64_key(b64: &str) -> CoreResult<Self> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(b64.trim()))
            .map_err(|_| CoreError::BadRequest("encryption key is not valid base64".into()))?;
        Self::from_key_bytes(&bytes)
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(Key::from_slice(self.key.as_ref()))
    }

    /// Encrypt UTF-8 plaintext into an `enc:v1:<base64(nonce||ciphertext)>` token.
    pub fn encrypt(&self, plaintext: &str) -> CoreResult<String> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = XNonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher()
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|_| CoreError::Crypto)?;
        let mut combined = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);
        Ok(format!(
            "{PREFIX}{}",
            base64::engine::general_purpose::STANDARD.encode(&combined)
        ))
    }

    /// Decrypt a token produced by [`Self::encrypt`]. A value without the
    /// `enc:v1:` prefix is treated as legacy plaintext and returned verbatim.
    pub fn decrypt(&self, token: &str) -> CoreResult<String> {
        let Some(b64) = token.strip_prefix(PREFIX) else {
            return Ok(token.to_string());
        };
        let combined = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|_| CoreError::Crypto)?;
        if combined.len() <= NONCE_LEN {
            return Err(CoreError::Crypto);
        }
        let (nonce_bytes, ciphertext) = combined.split_at(NONCE_LEN);
        let nonce = XNonce::from_slice(nonce_bytes);
        let plaintext = self
            .cipher()
            .decrypt(nonce, ciphertext)
            .map_err(|_| CoreError::Crypto)?;
        String::from_utf8(plaintext).map_err(|_| CoreError::Crypto)
    }

    /// `true` if `value` is a token this module produced (vs. legacy plaintext).
    pub fn is_encrypted(value: &str) -> bool {
        value.starts_with(PREFIX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher() -> SecretCipher {
        SecretCipher::from_key_bytes(&[7u8; KEY_LEN]).unwrap()
    }

    #[test]
    fn round_trips() {
        let c = cipher();
        let token = c.encrypt("super-secret-token").unwrap();
        assert!(token.starts_with(PREFIX));
        assert_ne!(token, "super-secret-token");
        assert_eq!(c.decrypt(&token).unwrap(), "super-secret-token");
    }

    #[test]
    fn nonce_makes_each_ciphertext_unique() {
        let c = cipher();
        assert_ne!(c.encrypt("x").unwrap(), c.encrypt("x").unwrap());
    }

    #[test]
    fn legacy_plaintext_passes_through() {
        let c = cipher();
        assert_eq!(c.decrypt("plain-old-value").unwrap(), "plain-old-value");
    }

    #[test]
    fn wrong_key_fails() {
        let token = cipher().encrypt("secret").unwrap();
        let other = SecretCipher::from_key_bytes(&[9u8; KEY_LEN]).unwrap();
        assert!(matches!(other.decrypt(&token), Err(CoreError::Crypto)));
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let c = cipher();
        let mut token = c.encrypt("secret").unwrap();
        // Flip the last base64 char.
        let last = token.pop().unwrap();
        token.push(if last == 'A' { 'B' } else { 'A' });
        assert!(c.decrypt(&token).is_err());
    }

    #[test]
    fn rejects_wrong_key_length() {
        assert!(SecretCipher::from_key_bytes(&[0u8; 16]).is_err());
    }
}
