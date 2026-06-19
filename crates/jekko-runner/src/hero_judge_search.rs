//! Search and evidence loading helpers for Hero/Judge runs.

use std::path::Path;
use std::sync::Arc;

use agent_search::{
    EvidencePolicy, ExtractionPolicy, ProviderCapabilities, ProviderEntry, ProviderId,
    ProviderPolicy, ProviderReceipt, ProviderSearchRequest, ProviderSearchResponse, QueryClass,
    ResearchLimits, ResearchRequest, SafetyPolicy, SearchConfig, SearchHit, SearchProvider,
};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use crate::evidence::{load_evidence_inputs, LoadedEvidence};
use crate::hashing::{sha256_hex, sha256_json};
use crate::hero_judge::{HeroJudgeConfig, HeroJudgeMissingProviderPolicy, HeroJudgeSearchReceipt};
use crate::port::{EvidenceInput, EvidenceInputKind};

pub(crate) async fn run_research(
    _repo: &Path,
    objective: &str,
    config: &HeroJudgeConfig,
    live_search: bool,
) -> Result<Vec<HeroJudgeSearchReceipt>> {
    if !config.research.enabled {
        return Ok(vec![HeroJudgeSearchReceipt {
            id: "search-disabled".to_string(),
            provider: "none".to_string(),
            query: String::new(),
            status: "skipped".to_string(),
            reason: Some("research_disabled".to_string()),
            url_count: 0,
            content_sha256: sha256_hex(b"research_disabled"),
        }]);
    }

    let queries = research_queries(objective, config);
    let use_live = live_search
        && config.research.live_when_available
        && std::env::var("AGENT_SEARCH_LIVE").ok().as_deref() == Some("1");
    let mut receipts = Vec::new();
    let (providers, skipped, defaults) = if use_live {
        let search_config = SearchConfig::from_env();
        if config.research.missing_provider == HeroJudgeMissingProviderPolicy::Fail
            && !search_config.skipped.is_empty()
        {
            anyhow::bail!(
                "hero/judge search provider missing: {}",
                search_config
                    .skipped
                    .iter()
                    .filter_map(|receipt| receipt.reason.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        (
            search_config.providers,
            search_config.skipped,
            (
                search_config.provider_policy,
                search_config.extraction,
                search_config.evidence,
                search_config.safety,
            ),
        )
    } else {
        if live_search && config.research.missing_provider == HeroJudgeMissingProviderPolicy::Fail {
            anyhow::bail!("hero/judge live search requested but AGENT_SEARCH_LIVE=1 is not set");
        }
        (
            vec![ProviderEntry::new(Arc::new(FixtureSearchProvider))],
            vec![ProviderReceipt::skipped(
                ProviderId::OpenAlex,
                "",
                "fixture_search_provider",
            )],
            (
                ProviderPolicy::default(),
                ExtractionPolicy::default(),
                EvidencePolicy::default(),
                SafetyPolicy::default(),
            ),
        )
    };

    for skipped in skipped {
        if !skipped.query.is_empty() {
            receipts.push(map_search_receipt(&skipped));
        }
    }

    for query in queries
        .into_iter()
        .take(config.budgets.search_queries.max(1))
    {
        let request = ResearchRequest {
            query: query.clone(),
            objective: Some(objective.to_string()),
            mode: QueryClass::Academic,
            providers: defaults.0.clone(),
            limits: ResearchLimits {
                max_queries: 1,
                max_pages: config.budgets.search_pages.clamp(1, 100),
                max_parallel: config.population.max_parallel.max(1),
                timeout_seconds: 30,
                max_cost_usd: 0.0,
            },
            extraction: defaults.1.clone(),
            evidence: defaults.2.clone(),
            safety: defaults.3.clone(),
        };
        let response =
            agent_search::search_parallel(providers.clone(), request, QueryClass::Academic).await;
        receipts.extend(response.receipts.iter().map(map_search_receipt));
    }
    if receipts.is_empty() {
        receipts.push(HeroJudgeSearchReceipt {
            id: "search-empty".to_string(),
            provider: "none".to_string(),
            query: String::new(),
            status: "skipped".to_string(),
            reason: Some("no_search_receipts".to_string()),
            url_count: 0,
            content_sha256: sha256_hex(b"no_search_receipts"),
        });
    }
    Ok(receipts)
}

pub(crate) fn load_hero_judge_evidence(
    repo: &Path,
    config: &HeroJudgeConfig,
) -> Result<Vec<LoadedEvidence>> {
    let inputs = if config.evidence.is_empty() {
        default_evidence_inputs(repo)
    } else {
        config
            .evidence
            .iter()
            .map(|input| EvidenceInput {
                id: input.id.clone(),
                kind: if input.path.contains('*') {
                    EvidenceInputKind::Glob
                } else {
                    EvidenceInputKind::File
                },
                role: input.role.clone(),
                path_or_url: input.path.clone(),
                max_bytes: input.max_bytes,
            })
            .collect()
    };
    load_evidence_inputs(repo, &inputs)
}

fn default_evidence_inputs(repo: &Path) -> Vec<EvidenceInput> {
    [
        ("zyal-loops", "workflow_doc", "docs/zyal-research-loops.md"),
        (
            "theory-admission",
            "theory_gate",
            "docs/theory-admission.md",
        ),
        (
            "benchmark-methodology",
            "benchmark_gate",
            "docs/benchmark-methodology.md",
        ),
        ("scoring", "scoring_gate", "docs/scoring.md"),
        ("tip1", "filtered_tip", "tips/rolling/tip1.txt"),
        ("tip2", "filtered_tip", "tips/rolling/tip2.txt"),
        ("example1", "filtered_tip", "tips/rolling/example1.txt"),
    ]
    .into_iter()
    .filter(|(_, _, path)| repo.join(path).is_file())
    .map(|(id, role, path)| EvidenceInput {
        id: id.to_string(),
        kind: EvidenceInputKind::File,
        role: role.to_string(),
        path_or_url: path.to_string(),
        max_bytes: 16 * 1024,
    })
    .collect()
}

fn map_search_receipt(receipt: &ProviderReceipt) -> HeroJudgeSearchReceipt {
    let status = match receipt.status {
        agent_search::ReceiptStatus::Ok => "ok",
        agent_search::ReceiptStatus::Skipped => "skipped",
        agent_search::ReceiptStatus::Failed => "failed",
    };
    let id_payload = json!({
        "provider": receipt.provider.as_str(),
        "query": receipt.query,
        "status": status,
        "reason": receipt.reason,
    });
    HeroJudgeSearchReceipt {
        id: format!(
            "search-{}",
            &sha256_json(&id_payload, "search_receipt")[..12]
        ),
        provider: receipt.provider.as_str().to_string(),
        query: receipt.query.clone(),
        status: status.to_string(),
        reason: receipt.reason.clone(),
        url_count: receipt.url_count,
        content_sha256: if receipt.content_hash.is_empty() {
            sha256_json(&id_payload, "search_receipt")
        } else {
            receipt.content_hash.clone()
        },
    }
}

fn research_queries(objective: &str, config: &HeroJudgeConfig) -> Vec<String> {
    if !config.research.queries.is_empty() {
        return config.research.queries.clone();
    }
    vec![
        format!("OpenQG theory admission evidence {objective}"),
        "quantum gravity benchmark methodology falsifiable predictions".to_string(),
        "prompt evolution hero judge verifier red team scientific claims".to_string(),
    ]
}

struct FixtureSearchProvider;

#[async_trait]
impl SearchProvider for FixtureSearchProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAlex
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(true, true, false, false, false, false, true)
    }

    async fn search(
        &self,
        req: ProviderSearchRequest,
    ) -> agent_search::Result<ProviderSearchResponse> {
        let hit = SearchHit::new(
            ProviderId::OpenAlex,
            format!("Fixture OpenQG evidence for {}", req.query),
            "https://example.invalid/openqg/fixture-evidence",
            Some("Deterministic offline fixture receipt for OpenQG hero/judge evolution.".into()),
            vec![format!(
                "fixture:{}",
                &sha256_hex(req.query.as_bytes())[..12]
            )],
        )?;
        let receipt =
            ProviderReceipt::ok(ProviderId::OpenAlex, &req.query, std::slice::from_ref(&hit));
        Ok(ProviderSearchResponse {
            hits: vec![hit],
            evidence: Vec::new(),
            receipts: vec![receipt],
            warnings: Vec::new(),
        })
    }
}
