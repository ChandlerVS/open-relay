//! Sealed upload receipts — the trust boundary for the unauthenticated
//! presign endpoint.
//!
//! # Why the submitted value can't just be a URL
//!
//! `POST /public/forms/{id}/uploads` has to be unauthenticated: it serves
//! embedded forms running on third-party host pages. So whatever the browser
//! sends back at submission time is attacker-controlled. If a `file` field's
//! value were a URL string, anyone could POST any URL and have it delivered
//! into a CRM record as though a visitor had uploaded it.
//!
//! Instead the presign handler seals what it just authorised:
//!
//! ```text
//! v1|<form_id>|<field_key>|<issued_at_unix>|<object_key>
//! ```
//!
//! and hands the client the ciphertext. [`seal`] uses the **existing**
//! [`crate::crypto::SecretCipher`] — XChaCha20-Poly1305 is an AEAD, so this is
//! a signature with no new key material, no new dependency and no new table.
//! [`open`] then re-derives the object key from the token and checks it was
//! minted for *this* form and *this* field, recently.
//!
//! Binding both `form_id` and `field_key` matters: without them a receipt for a
//! 1-byte upload to a throwaway form could be replayed into a different form's
//! field. The timestamp bounds how long a leaked receipt stays replayable.
//!
//! The field separator is `|`, which cannot appear in a `field_key` (keys
//! reject whitespace and control characters but not punctuation), so the parse
//! splits on the first four separators only and treats the remainder as the
//! object key — object keys contain `/` and may in principle contain `|`.

use chrono::Utc;

use crate::crypto::SecretCipher;
use crate::error::{CoreError, CoreResult};

/// How long a sealed receipt may be presented for. Generous relative to the
/// 5-minute upload URL: a visitor may attach a file early and spend a while
/// filling in a long multi-step form before submitting.
pub const RECEIPT_TTL_SECS: i64 = 24 * 60 * 60;

const VERSION: &str = "v1";

/// What a receipt attests to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadReceipt {
    pub form_id: i32,
    pub field_key: String,
    pub issued_at: i64,
    pub object_key: String,
}

/// Seal a receipt for an upload we just presigned.
pub fn seal(
    cipher: &SecretCipher,
    form_id: i32,
    field_key: &str,
    object_key: &str,
) -> CoreResult<String> {
    let issued_at = Utc::now().timestamp();
    cipher.encrypt(&format!(
        "{VERSION}|{form_id}|{field_key}|{issued_at}|{object_key}"
    ))
}

/// Open a receipt presented at submission time, verifying it was minted for
/// this form + field and hasn't expired.
///
/// Every failure mode collapses to one message on purpose: a submitter can't
/// act on the difference between "tampered", "expired" and "wrong field", and
/// distinguishing them would tell a prober which part of the token they got
/// right.
pub fn open(
    cipher: &SecretCipher,
    token: &str,
    form_id: i32,
    field_key: &str,
) -> Result<UploadReceipt, ReceiptError> {
    // A value that never went through `seal` won't carry the `enc:v1:` prefix,
    // and `SecretCipher::decrypt` passes those through verbatim as legacy
    // plaintext. Reject them here rather than letting a raw URL parse.
    if !SecretCipher::is_encrypted(token) {
        return Err(ReceiptError);
    }
    let plain = cipher.decrypt(token).map_err(|_| ReceiptError)?;

    let mut parts = plain.splitn(5, '|');
    let version = parts.next().ok_or(ReceiptError)?;
    let seen_form = parts.next().ok_or(ReceiptError)?;
    let seen_field = parts.next().ok_or(ReceiptError)?;
    let issued_at = parts.next().ok_or(ReceiptError)?;
    let object_key = parts.next().ok_or(ReceiptError)?;

    if version != VERSION {
        return Err(ReceiptError);
    }
    if seen_form.parse::<i32>().map_err(|_| ReceiptError)? != form_id {
        return Err(ReceiptError);
    }
    if seen_field != field_key {
        return Err(ReceiptError);
    }
    let issued_at: i64 = issued_at.parse().map_err(|_| ReceiptError)?;
    let age = Utc::now().timestamp() - issued_at;
    // A negative age (clock skew, or a token minted "in the future") is as
    // suspect as an expired one.
    if !(0..=RECEIPT_TTL_SECS).contains(&age) {
        return Err(ReceiptError);
    }
    if object_key.is_empty() {
        return Err(ReceiptError);
    }

    Ok(UploadReceipt {
        form_id,
        field_key: seen_field.to_string(),
        issued_at,
        object_key: object_key.to_string(),
    })
}

/// Opaque failure. Carries no detail by design (see [`open`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiptError;

impl ReceiptError {
    /// The message a submitter sees. Names the field so a multi-file form
    /// tells them which attachment to redo, but says nothing about why.
    pub fn into_core(self, field_key: &str) -> CoreError {
        CoreError::BadRequest(format!(
            "file upload for '{field_key}' is invalid or expired; please re-attach the file"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher() -> SecretCipher {
        SecretCipher::from_key_bytes(&[3u8; crate::crypto::KEY_LEN]).unwrap()
    }

    #[test]
    fn round_trips() {
        let c = cipher();
        let token = seal(&c, 7, "resume", "forms/7/2026/09/abc/cv.pdf").unwrap();
        let r = open(&c, &token, 7, "resume").unwrap();
        assert_eq!(r.object_key, "forms/7/2026/09/abc/cv.pdf");
        assert_eq!(r.field_key, "resume");
        assert_eq!(r.form_id, 7);
    }

    #[test]
    fn object_key_containing_separators_survives() {
        let c = cipher();
        let key = "forms/7/2026/09/abc/we|ird|name.pdf";
        let token = seal(&c, 7, "resume", key).unwrap();
        assert_eq!(open(&c, &token, 7, "resume").unwrap().object_key, key);
    }

    #[test]
    fn rejects_other_form() {
        let c = cipher();
        let token = seal(&c, 7, "resume", "k").unwrap();
        assert!(open(&c, &token, 8, "resume").is_err());
    }

    #[test]
    fn rejects_other_field() {
        let c = cipher();
        let token = seal(&c, 7, "resume", "k").unwrap();
        assert!(open(&c, &token, 7, "cover_letter").is_err());
    }

    #[test]
    fn rejects_plaintext_url() {
        let c = cipher();
        // The exact attack the receipt exists to stop: a bare URL submitted as
        // the field value. `SecretCipher::decrypt` would hand this back
        // verbatim as legacy plaintext, so `open` has to reject it up front.
        assert!(open(&c, "https://evil.example/x.pdf", 7, "resume").is_err());
    }

    #[test]
    fn rejects_tampered_token() {
        let c = cipher();
        let mut token = seal(&c, 7, "resume", "k").unwrap();
        let last = token.pop().unwrap();
        token.push(if last == 'A' { 'B' } else { 'A' });
        assert!(open(&c, &token, 7, "resume").is_err());
    }

    #[test]
    fn rejects_foreign_key() {
        let token = seal(&cipher(), 7, "resume", "k").unwrap();
        let other = SecretCipher::from_key_bytes(&[9u8; crate::crypto::KEY_LEN]).unwrap();
        assert!(open(&other, &token, 7, "resume").is_err());
    }

    #[test]
    fn rejects_expired() {
        let c = cipher();
        let stale = Utc::now().timestamp() - RECEIPT_TTL_SECS - 1;
        let token = c
            .encrypt(&format!("v1|7|resume|{stale}|forms/7/x.pdf"))
            .unwrap();
        assert!(open(&c, &token, 7, "resume").is_err());
    }

    #[test]
    fn rejects_future_dated() {
        let c = cipher();
        let future = Utc::now().timestamp() + 600;
        let token = c
            .encrypt(&format!("v1|7|resume|{future}|forms/7/x.pdf"))
            .unwrap();
        assert!(open(&c, &token, 7, "resume").is_err());
    }

    #[test]
    fn rejects_unknown_version() {
        let c = cipher();
        let now = Utc::now().timestamp();
        let token = c.encrypt(&format!("v2|7|resume|{now}|k")).unwrap();
        assert!(open(&c, &token, 7, "resume").is_err());
    }
}
