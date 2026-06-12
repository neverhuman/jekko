use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Semaphore;

use crate::model_policy::ModelTaskKind;

use super::{ModelCallReceipt, ModelClient};

/// Budgeting and live-receipt guard around another model client.
#[derive(Debug)]
pub struct BudgetedModelClient<C> {
    inner: C,
    max_calls: usize,
    used: AtomicUsize,
    semaphore: Arc<Semaphore>,
    require_live: bool,
}

impl<C> BudgetedModelClient<C> {
    /// Wrap an inner model client with a call budget and concurrency cap.
    pub fn new(inner: C, max_calls: usize, max_parallel: usize, require_live: bool) -> Self {
        Self {
            inner,
            max_calls: max_calls.max(1),
            used: AtomicUsize::new(0),
            semaphore: Arc::new(Semaphore::new(max_parallel.max(1))),
            require_live,
        }
    }

    /// Number of calls used so far.
    pub fn calls_used(&self) -> usize {
        self.used.load(Ordering::SeqCst)
    }

    /// Number of calls remaining.
    pub fn calls_remaining(&self) -> usize {
        self.max_calls.saturating_sub(self.calls_used())
    }
}

#[async_trait]
impl<C> ModelClient for BudgetedModelClient<C>
where
    C: ModelClient,
{
    async fn complete(
        &self,
        kind: ModelTaskKind,
        prompt: &str,
        cwd: &Path,
    ) -> Result<ModelCallReceipt> {
        self.complete_lane(kind, prompt, cwd, 0).await
    }

    async fn complete_lane(
        &self,
        kind: ModelTaskKind,
        prompt: &str,
        cwd: &Path,
        lane: usize,
    ) -> Result<ModelCallReceipt> {
        let Ok(_permit) = self.semaphore.clone().acquire_owned().await else {
            return Ok(ModelCallReceipt::failure(
                kind,
                "budget",
                "budget",
                "live call semaphore closed",
            ));
        };
        let previous = self.used.fetch_add(1, Ordering::SeqCst);
        if previous >= self.max_calls {
            self.used.store(self.max_calls, Ordering::SeqCst);
            let mut receipt = ModelCallReceipt::failure(
                kind,
                "budget",
                "budget",
                format!("live call budget exhausted at {} calls", self.max_calls),
            );
            receipt.budget_used = Some(self.max_calls);
            receipt.budget_remaining = Some(0);
            return Ok(receipt);
        }
        let used = previous + 1;
        let remaining = self.max_calls.saturating_sub(used);
        let mut receipt = self.inner.complete_lane(kind, prompt, cwd, lane).await?;
        receipt.budget_used = Some(used);
        receipt.budget_remaining = Some(remaining);
        if self.require_live && receipt.provider == "fake" {
            receipt.success = false;
            receipt.error = Some(
                "live model calls are required; deterministic model receipt rejected".to_string(),
            );
        }
        Ok(receipt)
    }
}
