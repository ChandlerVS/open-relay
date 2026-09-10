//! Object storage: the pluggable surface that holds files submitted through a
//! form's `file` field (S3 and S3-compatible stores today).
//!
//! Mirrors [`crate::backend`] deliberately — a [`FileStoreFactory`] is built
//! from a `storage_provider` DB row exactly the way a `BackendFactory` is built
//! from a `backend_instance` row, and [`FileStoreFactory::secret_keys`] plays
//! the same three-jobs-from-one-declaration role: DTO redaction, preservation
//! across partial updates, and encrypt/decrypt at rest.
//!
//! Unlike backends there is no static-singleton flavour: every store needs
//! credentials, so every kind is factory-built.
//!
//! # The upload flow, and why the server never sees the bytes
//!
//! The public API surface caps request bodies at 64 KB (embedded forms post
//! small JSON), so uploads do **not** pass through it. Instead the browser asks
//! for a short-lived presigned `PUT` ([`FileStore::presign_put`]) and uploads
//! straight to the bucket. `content_type` and `content_length` are signed into
//! that URL, which is what actually enforces the field's size limit — the store
//! rejects a body that doesn't match the signature.
//!
//! The submitted field value is then a sealed [`receipt`] token rather than a
//! URL, because the presign endpoint is unauthenticated and a client-supplied
//! URL string would let anyone write anything into a delivered CRM field.

pub mod receipt;
pub mod registry;
pub mod s3;
pub mod uploads;

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use thiserror::Error;

pub use registry::{StorageKindInfo, StorageRegistry};
pub use s3::{S3Config, S3Factory, S3Visibility};
pub use uploads::{UploadTicketDto, UploadTicketRequest};

/// How long a presigned upload URL stays valid. Short: the browser PUTs
/// immediately after receiving it, and a leaked URL is a write into someone
/// else's bucket.
pub const UPLOAD_URL_TTL_SECS: u64 = 300;

/// Hard ceiling on a single upload, regardless of what a form's field config
/// asks for. A field may lower this; it may never raise it.
pub const MAX_UPLOAD_MB: u32 = 100;

/// Default per-field size cap when an author doesn't set one.
pub const DEFAULT_MAX_FILE_MB: u32 = 10;

pub const BYTES_PER_MB: u64 = 1024 * 1024;

/// A presigned upload the browser can execute directly against the store.
#[derive(Debug, Clone)]
pub struct PresignedUpload {
    pub url: String,
    /// Headers the client MUST send verbatim — they are part of the signature.
    pub headers: BTreeMap<String, String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum StorageError {
    /// The store is configured but unreachable / rejected us. Surfaces as a 502.
    #[error("storage unavailable: {0}")]
    Unavailable(String),
    /// The request can't be signed as asked (bad key, oversized TTL, …).
    #[error("storage request invalid: {0}")]
    Invalid(String),
}

/// Returned by a factory when the stored config can't be parsed/validated.
#[derive(Debug, Error)]
pub enum StorageBuildError {
    #[error("invalid storage config: {0}")]
    Invalid(String),
}

impl From<StorageBuildError> for StorageError {
    fn from(e: StorageBuildError) -> Self {
        Self::Invalid(e.to_string())
    }
}

#[async_trait]
pub trait FileStore: Send + Sync + 'static {
    /// Stable kind identifier, shared with the `storage_provider.kind` column.
    fn kind(&self) -> &'static str;

    /// Presign a browser `PUT` for `key`.
    ///
    /// `content_type` and `content_length` are signed into the URL, so a client
    /// cannot upload a larger or differently-typed object than it declared.
    /// This is the size enforcement, not a courtesy check.
    async fn presign_put(
        &self,
        key: &str,
        content_type: &str,
        content_length: u64,
    ) -> Result<PresignedUpload, StorageError>;

    /// The URL to persist on the submission once the object exists. Whether
    /// this is permanent or expiring is the provider's `visibility` setting.
    async fn stored_url(&self, key: &str) -> Result<String, StorageError>;

    /// Round-trip a small object to prove the credentials and bucket work.
    /// Powers the admin's "Test connection" button, so failures should carry a
    /// message an admin can act on.
    async fn check(&self) -> Result<(), StorageError>;
}

/// Builds a configured [`FileStore`] from a stored `storage_provider` row.
pub trait FileStoreFactory: Send + Sync + 'static {
    /// Stable kind identifier shared with the `storage_provider.kind` column.
    fn kind(&self) -> &'static str;

    /// Human label for the admin's provider picker.
    fn label(&self) -> &'static str;

    /// Top-level keys in this kind's `config` JSON that hold secrets. These are
    /// redacted from admin-facing DTOs, preserved across partial updates that
    /// omit them, and encrypted at rest. Default: none.
    fn secret_keys(&self) -> &'static [&'static str] {
        &[]
    }

    /// Parse + validate the row's `config` JSON and yield a ready-to-call
    /// store. Called on every write (as validation) and on the submission path,
    /// so implementations should stay cheap — clone a shared `reqwest::Client`
    /// rather than build a new pool.
    fn build(&self, config: &Value) -> Result<Arc<dyn FileStore>, StorageBuildError>;
}
