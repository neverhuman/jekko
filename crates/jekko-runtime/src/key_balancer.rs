//! Smart load-balancer over `(provider, user, model)` key candidates.
//!
//! Wraps [`jekko_provider::key_pool::KeyPool`] with per-tuple usage counters
//! persisted in `~/.jekko/users/<user_id>/state.sqlite`. Selection is a
//! deterministic round-robin over currently eligible candidates.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use jekko_provider::adapter::ProviderCredential;
use jekko_provider::key_pool::{user_dir, KeyPool, STATE_DB_FILENAME};
use rusqlite::{Connection, OptionalExtension};

include!("key_balancer/health.rs");
include!("key_balancer/store.rs");
include!("key_balancer/balancer.rs");
include!("key_balancer/scoring.rs");

#[cfg(test)]
include!("key_balancer/tests.rs");
