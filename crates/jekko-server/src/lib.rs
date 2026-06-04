//! Jekko HTTP/WebSocket/PTY/SSE server.
//!
//! Phase 6 of the legacy JS -> Rust port. Mirrors the externally-observable
//! surface of `packages/jekko/src/server/`:
//!
//! - [`app::router`] builds the Axum app.
//! - [`state::AppState`] bundles long-lived runtime services.
//! - [`auth`] / [`cors`] host middleware.
//! - [`routes`] hosts per-group handlers.
//! - [`serve`] runs the assembled app on a [`tokio::net::TcpListener`].
//!
//! This crate only speaks HTTP, WebSocket, SSE, and PTY-over-WS; rendering
//! work lives in the sibling `jekko-tui` crate.
#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

pub mod app;
pub mod auth;
pub mod cors;
pub mod error;
pub mod memory_observer;
pub mod routes;
pub mod state;

pub use app::router;
pub use auth::AuthConfig;
pub use cors::CorsConfig;
pub use error::{ServerError, ServerResult};
pub use state::{AppState, DaemonRegistry, InstanceMeta, QuestionRegistry, WorkspaceRegistry};

/// On-disk HTTP API configuration retained for downstream crates that still
/// depend on this shape.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HttpApiConfig {
    /// Bind port.
    pub port: u16,
}

/// Convenience wrapper: bind `addr` and serve the supplied state.
pub async fn serve(addr: SocketAddr, state: Arc<AppState>) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    let app = router(state);
    axum::serve(listener, app).await?;
    Ok(())
}

/// Build the router with default state for callers that haven't migrated to
/// [`router`] yet.
pub fn build_router() -> Result<()> {
    let _ = router(Arc::new(AppState::default()));
    Ok(())
}
