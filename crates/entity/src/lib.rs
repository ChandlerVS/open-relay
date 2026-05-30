//! OpenRelay domain entities.
//!
//! Hand-authored using SeaORM 2.0's entity-first workflow. Do NOT run
//! `sea-orm-cli generate` against this crate — schema is derived from the
//! Rust types and synced into MySQL by the server's boot sequence
//! (`db.get_schema_registry("entity::*").sync(&db).await?`).
//!
//! Pattern for new entities (e.g. `pub mod user;` then `crates/entity/src/user.rs`):
//!
//! ```ignore
//! #[sea_orm::model]
//! #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
//! #[sea_orm(table_name = "user")]
//! pub struct Model {
//!     #[sea_orm(primary_key)]
//!     pub id: i32,
//!     #[sea_orm(unique)]
//!     pub email: String,
//!     #[sea_orm(has_many)]
//!     pub forms: HasMany<super::form::Entity>,
//! }
//! impl ActiveModelBehavior for ActiveModel {}
//! ```
//!
//! Each new module declared below is auto-discovered by schema-sync via the
//! `entity::*` glob — no central registration required.

// Resource modules — add as they are implemented:
pub mod external_identity;
pub mod form;
pub mod oauth_provider_config;
pub mod role;
pub mod role_permission;
pub mod submission;
pub mod submission_delivery;
pub mod user;
pub mod user_role;
// pub mod backend;
