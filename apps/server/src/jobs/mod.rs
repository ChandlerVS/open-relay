//! Server-side wiring for the submission delivery worker.
//!
//! Domain logic lives in `core::jobs::worker`; this module just hands the
//! worker the resources it needs at boot.

use std::sync::Arc;

use open_relay_core::backend::registry::BackendRegistry;
use open_relay_core::crypto::SecretCipher;
use open_relay_core::jobs::worker;
use sea_orm::DatabaseConnection;

pub fn spawn(db: DatabaseConnection, backends: BackendRegistry, cipher: Arc<SecretCipher>) {
    worker::spawn(db, backends, cipher);
}
