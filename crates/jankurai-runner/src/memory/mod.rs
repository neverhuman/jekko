//! Phase E2 retrieval-augmented memory module.
//!
//! Two halves:
//!
//! * [`embedder`] — `Embedder` trait plus a [`OpenAICompatibleEmbedder`]
//!   that POSTs to jnoccio-fusion's `/v1/embeddings` and a [`FakeEmbedder`]
//!   for tests.
//! * [`retrieval`] — [`retrieve_for_run`] embeds a query, scans promoted
//!   capsules, cosine-ranks them, and demotes same-run capsules to avoid
//!   self-poisoning. [`format_lessons_prompt_block`] renders the top-k as
//!   a `<prior_lessons>` block for injection into the reasoning system
//!   prompt.
//!
//! Phase F's reasoning orchestrator wires `retrieve_for_run` +
//! `format_lessons_prompt_block` into its retrieval-augmented prompt
//! builder; in cold-start environments (no embedder service), the
//! deterministic fake at `Gateway::embed` keeps the path live.

pub mod embedder;
pub mod retrieval;

pub use embedder::{Embedder, FakeEmbedder, OpenAICompatibleEmbedder};
pub use retrieval::{format_lessons_prompt_block, retrieve_for_run, RetrievalConfig};

#[cfg(test)]
mod tests;
