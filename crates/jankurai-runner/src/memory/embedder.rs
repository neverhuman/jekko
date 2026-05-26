//! Embedding clients for Phase E2 semantic retrieval.
//!
//! The Phase E2 runtime pre-embeds the query for each retrieval pass and
//! stores per-capsule embeddings on durable writes. The trait is a thin
//! async boundary so the orchestrator can swap between a real
//! OpenAI-compatible HTTP endpoint (talking to jnoccio-fusion's
//! `/v1/embeddings`) and a deterministic [`FakeEmbedder`] for tests.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

/// Produces a fixed-dim embedding for a string. `Send + Sync` so the trait
/// object can sit behind `&dyn Embedder` shared across the retrieval helper
/// and the orchestrator.
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Embed a single query string. Implementations should be deterministic
    /// when the input is identical so cosine ranking stays stable across
    /// runs (the fake honors that; real providers do too in practice).
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}

/// HTTP-backed [`Embedder`] that POSTs to an OpenAI-compatible
/// `/v1/embeddings` endpoint. The default constructor targets the local
/// jnoccio-fusion bind at `http://127.0.0.1:4317/v1/embeddings`, which
/// returns a deterministic 1536-dim fake when no real upstream embedding
/// model is configured (see `Gateway::embed`).
#[derive(Clone, Debug)]
pub struct OpenAICompatibleEmbedder {
    endpoint: String,
    model: String,
    client: reqwest::Client,
}

impl OpenAICompatibleEmbedder {
    pub fn new(endpoint: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Convenience constructor pointing at the local jnoccio-fusion bind.
    pub fn local_default() -> Self {
        Self::new(
            "http://127.0.0.1:4317/v1/embeddings",
            "text-embedding-3-small",
        )
    }

    /// Override the inner `reqwest::Client` (e.g. to supply a longer timeout
    /// or a custom resolver during tests).
    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingItem>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingItem {
    embedding: Vec<f32>,
}

#[async_trait]
impl Embedder for OpenAICompatibleEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let body = json!({
            "model": self.model,
            "input": text,
        });
        let response = self
            .client
            .post(&self.endpoint)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {}", self.endpoint))?;
        let status = response.status();
        if !status.is_success() {
            let body = match response.text().await {
                Ok(text) => text,
                Err(read_err) => format!("<unable to read body: {read_err}>"),
            };
            anyhow::bail!(
                "embeddings endpoint {} returned HTTP {status}: {body}",
                self.endpoint
            );
        }
        let parsed: EmbeddingResponse = response
            .json()
            .await
            .with_context(|| format!("decode response from {}", self.endpoint))?;
        let Some(first) = parsed.data.into_iter().next() else {
            anyhow::bail!("embeddings endpoint {} returned no data", self.endpoint);
        };
        Ok(first.embedding)
    }
}

/// Deterministic [`Embedder`] for tests — always returns the configured
/// vector regardless of input. Pair with custom inputs in unit tests to
/// validate cosine ranking and filtering paths without standing up a real
/// embedder.
#[derive(Clone, Debug)]
pub struct FakeEmbedder {
    vec: Vec<f32>,
}

impl FakeEmbedder {
    pub fn new(vec: Vec<f32>) -> Self {
        Self { vec }
    }
}

#[async_trait]
impl Embedder for FakeEmbedder {
    async fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Ok(self.vec.clone())
    }
}
