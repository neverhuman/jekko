//! Patent providers for ZYAL research — thin adapters over the self-contained `patent-search` crate
//! (jekko-zyal). The heavy logic (HTTP + normalisation + offline-tested parsers) lives there; this module
//! only converts `patent_search::PatentHit` -> agent-search `SearchHit` and implements the `SearchProvider`
//! trait so the existing router/policy machinery treats patents like any other provider.
//!
//! Tokens used in ZYAL `research.provider_policy.allow`: `patentsview`, `epo_ops`, `serpapi_patents`, `lens`.

use crate::types::*;
use async_trait::async_trait;

/// One adapter wrapping any `patent_search` backend behind agent-search's `SearchProvider`.
pub struct PatentBackend {
    inner: Box<dyn patent_search::PatentProvider>,
    id: ProviderId,
    requires_key: bool,
}

impl PatentBackend {
    pub fn patentsview() -> Self {
        Self {
            inner: Box::new(patent_search::PatentsViewProvider::new()),
            id: ProviderId::PatentsView,
            requires_key: false,
        }
    }
    pub fn epo_ops(key: String, secret: String) -> Self {
        Self {
            inner: Box::new(patent_search::EpoOpsProvider::new(Some(key), Some(secret))),
            id: ProviderId::EpoOps,
            requires_key: true,
        }
    }
    pub fn serpapi(api_key: String) -> Self {
        Self {
            inner: Box::new(patent_search::SerpApiPatentsProvider::new(Some(api_key))),
            id: ProviderId::SerpapiPatents,
            requires_key: true,
        }
    }
    pub fn lens(token: String) -> Self {
        Self {
            inner: Box::new(patent_search::LensProvider::new(Some(token))),
            id: ProviderId::Lens,
            requires_key: true,
        }
    }
}

#[async_trait]
impl SearchProvider for PatentBackend {
    fn id(&self) -> ProviderId {
        self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        // academic-grade structured source; not web/news/code; official APIs => privacy_first.
        ProviderCapabilities::new(false, true, false, false, false, self.requires_key, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let query = patent_search::PatentQuery {
            query: req.query.clone(),
            max_results: req.limit.max(1) as u32,
            country: None,
            date_from: None,
        };
        let resp = self
            .inner
            .search(&query)
            .await
            .map_err(|e| SearchError::Request(e.to_string()))?;

        let mut out = ProviderSearchResponse::default();
        if resp.source == "skip_with_receipt" {
            out.receipts.push(ProviderReceipt::skipped(
                self.id,
                &req.query,
                &resp
                    .receipt
                    .unwrap_or_else(|| "provider not configured".into()),
            ));
            return Ok(out);
        }
        for h in resp.hits {
            let citation = vec![format!("patent:{}", h.patent_id)];
            out.hits.push(SearchHit::new(
                self.id, h.title, h.url, h.snippet, citation,
            )?);
        }
        out.receipts
            .push(ProviderReceipt::ok(self.id, &req.query, &out.hits));
        Ok(out)
    }
}
