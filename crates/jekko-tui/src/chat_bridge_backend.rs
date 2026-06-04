//! `ChatBackend` adapter for the existing SSE-based chat bridge worker.
//!
//! The worker in [`crate::chat_bridge`] streams [`crate::action::Action`]s back
//! through an `mpsc::Sender<Action>`; the inline runtime speaks the
//! [`ChatEvent`] vocabulary instead. This module bridges the two with a
//! translator thread per turn so the inline runtime can stay free of
//! chat-bridge implementation details.
//!
//! ## Diff routing (T1-V5b)
//!
//! `RuntimeEvent` carries no structured diff payload today, so we accumulate
//! the stdout of each in-flight tool keyed by `id` and, on `ToolEvent::Complete`,
//! try to parse the buffer as a unified diff. When parsing yields one or more
//! files, we emit a [`ChatEvent::Diff`] per file ahead of forwarding the
//! `Complete` so the inline runtime renders the diff card immediately. The
//! `Complete` chip render still fires as usual.
//!
//! See COWBOY follow-up T-??? — pushing structured diff data through
//! `RuntimeEvent::Diff` (or a `ToolEvent::Diff`) would let us skip the
//! string-parsing heuristic entirely.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use crate::action::{Action, RuntimeEvent, ToolEvent};
use crate::chat_bridge::spawn_chat_request;
use crate::engine::cancel::CancellationToken;
use crate::inline_runtime::{ChatBackend, ChatEvent, DiffBlockLine};
use crate::transcript::diff::{parse_unified_diff, DiffFile, DiffLineKind as ParserDiffLineKind};
use crate::transcript::inline_cards::{DiffLineKind, NoticeKind};

include!("chat_bridge_backend/config.rs");
include!("chat_bridge_backend/translate.rs");

#[cfg(test)]
include!("chat_bridge_backend/tests.rs");
