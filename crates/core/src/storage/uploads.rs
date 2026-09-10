//! Minting an upload ticket: everything between "a visitor picked a file" and
//! "the browser may PUT it to the bucket".
//!
//! This is the one write path into an operator's object store that is reachable
//! without authentication — embedded forms run on third-party host pages, so
//! the endpoint in front of it cannot require a token. The controls therefore
//! have to be read as a set, and each one is doing a distinct job:
//!
//! - The named field must exist on the form **and** be a `file` field. There
//!   is no way to ask for an arbitrary object key; keys are constructed here.
//! - The declared size must fit the field's cap, itself clamped by
//!   [`super::MAX_UPLOAD_MB`] so a hand-edited row can't raise the ceiling.
//! - The declared content type must satisfy the field's `accept` list.
//! - `Content-Type` and `Content-Length` are then *signed into* the presigned
//!   URL, so the declaration isn't merely trusted — a client that uploads
//!   something bigger or of another type fails the store's signature check.
//! - The key embeds a UUID, so one visitor can't overwrite another's file, and
//!   nothing is guessable.
//! - The URL expires in [`super::UPLOAD_URL_TTL_SECS`].
//!
//! What is deliberately *not* here: any record that the ticket was issued. An
//! object that is presigned and never submitted is an orphan indistinguishable
//! from a live one without a table, and the answer to that is an object
//! lifecycle rule on the prefix (the admin UI says so). Adding a row per
//! ticket would make an unauthenticated endpoint into an unauthenticated
//! `INSERT`, which is a worse trade.

use std::sync::Arc;

use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{BYTES_PER_MB, FileStore, MAX_UPLOAD_MB, receipt};
use crate::crypto::SecretCipher;
use crate::error::{CoreError, CoreResult};
use crate::forms::{CustomFieldType, FormElement};

/// Longest filename we keep. The name is cosmetic — it only makes the object
/// recognisable in a bucket listing — so truncating is harmless, while an
/// unbounded one would blow past S3's 1,024-byte key limit.
const MAX_FILENAME_LEN: usize = 100;

/// What the browser asks for once a visitor picks a file.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct UploadTicketRequest {
    /// Key of the `file` field being filled in.
    pub field_key: String,
    pub filename: String,
    pub content_type: String,
    /// Exact byte length. Signed into the URL, so it has to be honest.
    pub size: u64,
}

/// What it gets back.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UploadTicketDto {
    pub upload_url: String,
    pub method: String,
    /// Headers the client must send verbatim on the `PUT` — they are part of
    /// the signature.
    pub headers: std::collections::BTreeMap<String, String>,
    /// Opaque sealed receipt. Submit this as the field's value; the server
    /// exchanges it for the stored URL.
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

/// Validate a ticket request against the form's layout and mint the presigned
/// upload plus its receipt.
pub async fn issue_ticket(
    cipher: &SecretCipher,
    store: &Arc<dyn FileStore>,
    form_id: i32,
    layout: &[FormElement],
    req: &UploadTicketRequest,
) -> CoreResult<UploadTicketDto> {
    let field_key = req.field_key.trim();
    let field = layout
        .iter()
        .find_map(|el| match el {
            FormElement::Custom(c) if c.key == field_key && c.kind.is_file() => Some(c),
            _ => None,
        })
        .ok_or_else(|| {
            CoreError::BadRequest(format!("'{field_key}' is not a file field on this form"))
        })?;

    let CustomFieldType::File { accept, .. } = &field.kind else {
        unreachable!("filtered on is_file above");
    };
    // `max_upload_bytes` applies the global clamp, so a stored `max_size_mb`
    // above the ceiling silently reads as the ceiling rather than honouring it.
    let max_bytes = field
        .kind
        .max_upload_bytes()
        .unwrap_or(MAX_UPLOAD_MB as u64 * BYTES_PER_MB);
    if req.size == 0 {
        return Err(CoreError::BadRequest("the file is empty".into()));
    }
    if req.size > max_bytes {
        return Err(CoreError::BadRequest(format!(
            "file is too large — the limit for '{field_key}' is {} MB",
            max_bytes / BYTES_PER_MB
        )));
    }

    let content_type = normalise_content_type(&req.content_type);
    if !accept_matches(accept, &req.filename, &content_type) {
        return Err(CoreError::BadRequest(format!(
            "that file type isn't accepted for '{field_key}'"
        )));
    }

    let object_key = object_key_for(form_id, &req.filename, Utc::now());
    let presigned = store
        .presign_put(&object_key, &content_type, req.size)
        .await
        .map_err(|e| CoreError::Internal(anyhow::anyhow!("could not presign the upload: {e}")))?;

    // Seal the *unprefixed* key: `stored_url` applies the provider's prefix on
    // the way back out, so sealing the prefixed form would double it.
    let token = receipt::seal(cipher, form_id, field_key, &object_key)?;

    Ok(UploadTicketDto {
        upload_url: presigned.url,
        method: "PUT".to_string(),
        headers: presigned.headers,
        token,
        expires_at: presigned.expires_at,
    })
}

/// Build the object key. Namespaced by form and date so a bucket stays
/// browsable and a lifecycle rule can target a prefix, and by UUID so two
/// visitors uploading `resume.pdf` can't collide or overwrite each other.
fn object_key_for(form_id: i32, filename: &str, now: DateTime<Utc>) -> String {
    format!(
        "forms/{form_id}/{:04}/{:02}/{}/{}",
        now.year(),
        now.month(),
        uuid::Uuid::new_v4().simple(),
        safe_filename(filename)
    )
}

/// Reduce a client-supplied filename to something safe to append to a key.
///
/// Takes the basename (a client may send a full path), drops anything that
/// isn't plainly safe in a URL path segment, and truncates. Never returns an
/// empty string — the key must not end in `/`, which some stores treat as a
/// directory marker.
fn safe_filename(raw: &str) -> String {
    let base = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(raw)
        .trim()
        .trim_matches('.');
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '-' | '_' => c,
            _ => '-',
        })
        .collect();
    // Collapse runs of '-' so a name of mostly-unsafe characters doesn't turn
    // into a long dash string.
    let mut out = String::with_capacity(cleaned.len());
    let mut last_dash = false;
    for c in cleaned.chars() {
        if c == '-' {
            if !last_dash {
                out.push(c);
            }
            last_dash = true;
        } else {
            out.push(c);
            last_dash = false;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        return "upload".to_string();
    }
    if out.len() <= MAX_FILENAME_LEN {
        return out;
    }
    // Truncate from the front so the extension survives — it is the part that
    // makes the object openable.
    match out.rsplit_once('.') {
        Some((stem, ext)) if ext.len() < 16 => {
            let keep = MAX_FILENAME_LEN.saturating_sub(ext.len() + 1);
            format!("{}.{ext}", &stem[..keep.min(stem.len())])
        }
        _ => out[..MAX_FILENAME_LEN].to_string(),
    }
}

/// Lower-case and strip any `; charset=…` parameter, so the value that gets
/// signed matches what a browser will actually send.
fn normalise_content_type(raw: &str) -> String {
    let base = raw.split(';').next().unwrap_or(raw).trim().to_ascii_lowercase();
    if base.is_empty() {
        // What a browser reports for a file it can't classify. Signing an
        // empty Content-Type would produce a request the browser can't match.
        return "application/octet-stream".to_string();
    }
    base
}

/// Does this file satisfy the field's `accept` list? Implements the HTML
/// `accept` grammar we support: an extension (`.pdf`), a wildcard type
/// (`image/*`), or a full MIME type. An empty list accepts anything.
///
/// This is a *courtesy* check, not a security boundary — a client controls
/// both the filename and the declared content type. It exists so an honest
/// visitor gets a clear error instead of a signature failure from S3.
fn accept_matches(accept: &[String], filename: &str, content_type: &str) -> bool {
    if accept.is_empty() {
        return true;
    }
    let name = filename.to_ascii_lowercase();
    accept.iter().any(|pattern| {
        let p = pattern.trim().to_ascii_lowercase();
        if let Some(prefix) = p.strip_suffix("/*") {
            content_type
                .split_once('/')
                .is_some_and(|(base, _)| base == prefix)
        } else if p.starts_with('.') {
            name.ends_with(&p)
        } else {
            content_type == p
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn safe_filename_takes_the_basename() {
        assert_eq!(safe_filename("/etc/passwd"), "passwd");
        assert_eq!(safe_filename("C:\\Users\\a\\cv.pdf"), "cv.pdf");
    }

    #[test]
    fn safe_filename_neutralises_traversal_and_separators() {
        // The key is built by interpolation, so a name that could climb out of
        // its prefix would land somewhere the form doesn't own.
        assert_eq!(safe_filename("../../secret.pdf"), "secret.pdf");
        assert_eq!(safe_filename("..."), "upload");
        assert_eq!(safe_filename("a/b/../c.pdf"), "c.pdf");
        assert!(!safe_filename("a b?c=d#e.pdf").contains(['?', '#', '=', ' ']));
    }

    #[test]
    fn safe_filename_never_empty() {
        assert_eq!(safe_filename(""), "upload");
        assert_eq!(safe_filename("???"), "upload");
        assert_eq!(safe_filename("   "), "upload");
    }

    #[test]
    fn safe_filename_truncates_but_keeps_the_extension() {
        let long = format!("{}.pdf", "a".repeat(400));
        let out = safe_filename(&long);
        assert!(out.len() <= MAX_FILENAME_LEN, "{}", out.len());
        assert!(out.ends_with(".pdf"), "{out}");
    }

    #[test]
    fn object_key_is_namespaced_and_unique() {
        let when = Utc.with_ymd_and_hms(2026, 9, 10, 0, 0, 0).unwrap();
        let a = object_key_for(42, "cv.pdf", when);
        let b = object_key_for(42, "cv.pdf", when);
        assert!(a.starts_with("forms/42/2026/09/"), "{a}");
        assert!(a.ends_with("/cv.pdf"));
        assert_ne!(a, b, "two uploads of the same name must not collide");
    }

    #[test]
    fn accept_empty_allows_anything() {
        assert!(accept_matches(&[], "x.exe", "application/octet-stream"));
    }

    #[test]
    fn accept_matches_extension_wildcard_and_exact() {
        let accept = vec![".pdf".to_string(), "image/*".to_string()];
        assert!(accept_matches(&accept, "CV.PDF", "application/pdf"));
        assert!(accept_matches(&accept, "photo.png", "image/png"));
        assert!(!accept_matches(&accept, "notes.txt", "text/plain"));

        let exact = vec!["text/csv".to_string()];
        assert!(accept_matches(&exact, "data.csv", "text/csv"));
        assert!(!accept_matches(&exact, "data.csv", "text/plain"));
    }

    #[test]
    fn content_type_is_normalised() {
        assert_eq!(normalise_content_type("Text/CSV; charset=utf-8"), "text/csv");
        assert_eq!(normalise_content_type("  "), "application/octet-stream");
    }
}
