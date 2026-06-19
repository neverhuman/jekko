use crate::evidence::LoadedEvidence;

pub(super) fn evidence_topics(evidence: &[LoadedEvidence], limit: usize) -> Vec<String> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for item in evidence {
        for word in item
            .content
            .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
            .map(|word| word.trim_matches('_').to_ascii_lowercase())
        {
            if word.len() < 4 || is_stopword(&word) || word.chars().all(|ch| ch.is_ascii_digit()) {
                continue;
            }
            *counts.entry(word).or_insert(0) += 1;
        }
    }
    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_word, left_count), (right_word, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_word.cmp(right_word))
    });
    ranked
        .into_iter()
        .take(limit)
        .map(|(word, _)| word)
        .collect()
}

pub(super) fn command_tokens(evidence: &[LoadedEvidence]) -> Vec<String> {
    let mut commands = Vec::new();
    for item in evidence {
        for token in item
            .content
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .filter(|token| {
                (2..=16).contains(&token.len())
                    && token.chars().any(|ch| ch.is_ascii_alphabetic())
                    && token
                        .chars()
                        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
            })
        {
            let token = token.to_string();
            if !commands.contains(&token) {
                commands.push(token);
            }
        }
    }
    if commands.is_empty() {
        commands.push("PING".to_string());
    }
    commands
}

pub(super) fn string_field(value: &serde_json::Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        value
            .get(*name)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    })
}

pub(super) fn string_array_field(value: &serde_json::Value, name: &str) -> Option<Vec<String>> {
    let values = value.get(name)?.as_array()?;
    let strings = values
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    (!strings.is_empty()).then_some(strings)
}

pub(super) fn bool_field(value: &serde_json::Value, name: &str) -> Option<bool> {
    value.get(name)?.as_bool()
}

pub(super) fn slug(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "item".to_string()
    } else {
        out
    }
}

fn is_stopword(word: &str) -> bool {
    matches!(
        word,
        "about"
            | "after"
            | "before"
            | "build"
            | "client"
            | "clients"
            | "command"
            | "commands"
            | "component"
            | "components"
            | "define"
            | "design"
            | "evidence"
            | "from"
            | "implementation"
            | "important"
            | "should"
            | "stage"
            | "stages"
            | "system"
            | "target"
            | "tests"
            | "this"
            | "with"
            | "would"
    )
}
