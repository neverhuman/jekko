use crate::evidence::LoadedEvidence;
use crate::port::PortTargetRequest;

pub(crate) fn evidence_prompt_fragment(evidence: &[LoadedEvidence]) -> String {
    if evidence.is_empty() {
        return "no external evidence inputs configured".to_string();
    }
    evidence
        .iter()
        .map(|item| {
            let excerpt = item.content.chars().take(1200).collect::<String>();
            format!(
                "[{} role={} source={} bytes={} clipped={}]\n{}",
                item.id, item.role, item.source, item.bytes_read, item.clipped, excerpt
            )
        })
        .collect::<Vec<_>>()
        .join("\n---\n")
}

pub(crate) fn benchmark_prompt(target: &PortTargetRequest, evidence: &[LoadedEvidence]) -> String {
    format!(
        "Return JSON. Reconcile conflicting architecture evidence into a clean-room, parity-first execution plan for {} -> {}. Include evidence coverage, unsupported claims, parity cases, Jankurai/proof integration, and monitorability.\n{}",
        target.target,
        target.replacement,
        evidence_prompt_fragment(evidence)
    )
}
