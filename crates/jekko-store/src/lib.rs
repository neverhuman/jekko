//! SQLite persistence layer for Jekko.
//!
//! Mirrors the storage schema and per-domain SQL files:
//! - [`db::Db`] is the connection wrapper. [`db::Db::open`] applies the same
//!   pragma set and migration journal the earlier runtime used.
//! - [`migration`] hosts the Drizzle-compatible hasher + statement splitter
//!   and the `apply_journal` driver. The hash is byte-identical to the
//!   `migrationHash()` so existing databases open cleanly under Rust.
//! - The remaining modules are per-table repositories that take a borrowed
//!   [`rusqlite::Connection`] and return typed rows.
//!
//! All public types derive `Debug` + `Clone` (and `Serialize`/`Deserialize`
//! where the schema includes free-form JSON columns).
#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

pub mod account;
pub mod cogcore_wal {
    //! Compatibility alias for the memory WAL storage module.
    pub use crate::memory_wal::*;
}
pub mod daemon;
pub mod db;
pub mod error;
pub mod memory_wal;
pub mod migration;
pub mod project;
pub mod session;
pub mod share;
pub mod sync;
pub mod workspace;

pub use db::Db;
pub use error::{StoreError, StoreResult};
pub use migration::{
    apply_journal, migration_hash, migration_timestamp, split_statements, MigrationEntry,
    MIGRATIONS_TABLE,
};
