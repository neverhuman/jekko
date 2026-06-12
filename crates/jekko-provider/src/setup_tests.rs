use super::*;
use std::collections::BTreeMap;

fn env(values: &[(&str, &str)]) -> BTreeMap<String, EnvValue> {
    values
        .iter()
        .map(|(k, v)| {
            (
                (*k).to_string(),
                EnvValue {
                    value: Some((*v).to_string()),
                    source: Some(ModelKeySource::ProcessEnv),
                },
            )
        })
        .collect()
}

#[test]
fn catalog_is_dense() {
    assert!(CATALOG.iter().any(|e| e.provider_id == "openai"));
    assert!(CATALOG.iter().any(|e| e.provider_id == "anthropic"));
    assert!(CATALOG.iter().any(|e| e.provider_id == "jnoccio"));
    let scenario = catalog_entry("scripted_agent").unwrap();
    assert_eq!(scenario.recommended_model_id, Some("basic"));
    let cf = catalog_entry("cloudflare").unwrap();
    assert_eq!(cf.companion_env_names.unwrap().len(), 2);
}

#[test]
fn picks_highest_priority_when_multi_configured() {
    // OpenAI(90) + Anthropic(88) configured -> openai wins.
    let values = env(&[("OPENAI_API_KEY", "x"), ("ANTHROPIC_API_KEY", "y")]);
    let sel = choose_active_provider(&values, false);
    assert_eq!(sel.active_provider_id.as_deref(), Some("openai"));
}

#[test]
fn jekko_outranks_anthropic_when_both_present() {
    // Jekko(95) beats Anthropic(88).
    let values = env(&[("JEKKO_API_KEY", "x"), ("ANTHROPIC_API_KEY", "y")]);
    let sel = choose_active_provider(&values, false);
    assert_eq!(sel.active_provider_id.as_deref(), Some("jekko"));
}

#[test]
fn jnoccio_skipped_when_developer_locked() {
    // Without developer_unlocked, jnoccio is filtered out entirely.
    let values = env(&[("JNOCCIO_DEVELOPER_KEY", "secret")]);
    let sel = choose_active_provider(&values, false);
    assert!(sel.active_provider_id.is_none());
    let sel2 = choose_active_provider(&values, true);
    assert_eq!(sel2.active_provider_id.as_deref(), Some("jnoccio"));
}

#[test]
fn cloudflare_requires_companion() {
    let values = env(&[("CLOUDFLARE_API_TOKEN", "x")]);
    let sel = choose_active_provider(&values, false);
    assert!(sel.active_provider_id.is_none());

    let values = env(&[
        ("CLOUDFLARE_API_TOKEN", "x"),
        ("CLOUDFLARE_ACCOUNT_ID", "y"),
    ]);
    let sel = choose_active_provider(&values, false);
    assert_eq!(sel.active_provider_id.as_deref(), Some("cloudflare"));
}
