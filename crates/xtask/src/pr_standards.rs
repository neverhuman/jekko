use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use std::fs;
use std::process::Command as ProcessCommand;

use crate::current_github_event_context;

const DEFAULT_BRANCH: &str = "main";
const EMPTY_BODY_DEFAULT: &str = "";

pub fn run() -> Result<()> {
    let context = current_github_event_context()?;
    let repo = std::env::var("GITHUB_REPOSITORY").context("reading GITHUB_REPOSITORY")?;
    let number = pr_number(&context)?;
    let login = pull_request_field(&context, "pull_request.author.login")?;
    let title = pull_request_field(&context, "pull_request.title")?;
    let body = match pull_request_field(&context, "pull_request.body") {
        Ok(value) => value,
        Err(_) => EMPTY_BODY_DEFAULT.to_string(),
    };
    let created_at = parse_timestamp(&pull_request_field(&context, "pull_request.created_at")?)?;
    let default_branch = match context
        .field("repository.default_branch")
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => DEFAULT_BRANCH.to_string(),
    };

    if created_at < cutoff() {
        return Ok(());
    }
    if login == "jekko-agent[bot]" || is_team_member(&login)? {
        return Ok(());
    }

    let mut issues = Vec::new();
    if !has_section(&body, "### What does this PR do?")
        || !has_section(&body, "### Type of change")
        || !has_section(&body, "### How did you verify your code works?")
        || !has_section(&body, "### Checklist")
        || !has_section(&body, "### Issue for this PR")
    {
        issues.push(format!(
            "PR description is missing required template sections. Please use the [PR template](../blob/{default_branch}/.github/pull_request_template.md)."
        ));
    }

    if let Some(section) = extract_section(&body, "### What does this PR do?") {
        let sample_text = "Please provide a description of the issue";
        let trimmed = section.trim();
        if trimmed.is_empty()
            || (trimmed.contains(sample_text)
                && trimmed
                    .replace(sample_text, "")
                    .trim_matches(|c: char| c.is_whitespace() || c == '*')
                    .is_empty())
        {
            issues.push(
                "\"What does this PR do?\" section is empty or only contains sample text. Please describe your changes."
                    .to_string(),
            );
        }
    }

    if let Some(section) = extract_section(&body, "### Type of change") {
        if !section.to_lowercase().contains("- [x]") {
            issues.push(
                "No \"Type of change\" checkbox is checked. Please select at least one."
                    .to_string(),
            );
        }
    }

    if !title_is_exempt(&title) {
        if let Some(section) = extract_section(&body, "### Issue for this PR") {
            if !contains_issue_reference(&section) {
                issues.push(
                    "No issue referenced. Please add `Closes #<number>` linking to the relevant issue."
                        .to_string(),
                );
            }
        }
    }

    if let Some(section) = extract_section(&body, "### How did you verify your code works?") {
        if section.trim().is_empty() {
            issues.push(
                "\"How did you verify your code works?\" section is empty. Please explain how you tested."
                    .to_string(),
            );
        }
    }

    if let Some(section) = extract_section(&body, "### Checklist") {
        if !section.to_lowercase().contains("- [x]") {
            issues.push("At least one checklist item must be checked.".to_string());
        }
    }

    if issues.is_empty() {
        let _ = gh_api([
            "--method",
            "DELETE",
            &format!("/repos/{repo}/issues/{number}/labels/needs:issue"),
        ]);
        return Ok(());
    }

    let mut comment = String::from("PR review found the following issues:\n\n");
    for issue in &issues {
        comment.push_str("- ");
        comment.push_str(issue);
        comment.push('\n');
    }
    comment.push_str(&format!(
        "\nSee [CONTRIBUTING.md](../blob/{default_branch}/CONTRIBUTING.md#pr-titles) and [.github/pull_request_template.md](../blob/{default_branch}/.github/pull_request_template.md)."
    ));

    gh_api([
        "--method",
        "POST",
        &format!("/repos/{repo}/issues/{number}/comments"),
        "-f",
        &format!("body={comment}"),
    ])?;
    gh_api([
        "--method",
        "POST",
        &format!("/repos/{repo}/issues/{number}/labels"),
        "-f",
        "labels=[\"needs:issue\"]",
    ])?;
    Ok(())
}

fn cutoff() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-02-19T00:00:00Z")
        .expect("valid cutoff")
        .with_timezone(&Utc)
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("parse RFC3339 timestamp: {value}"))?
        .with_timezone(&Utc))
}

fn pr_number(context: &jekko_core::github::GitHubEventContext) -> Result<u64> {
    context
        .field("pull_request.number")
        .context("missing pull_request.number")?
        .parse()
        .context("parse pull_request.number")
}

fn pull_request_field(
    context: &jekko_core::github::GitHubEventContext,
    field: &str,
) -> Result<String> {
    context
        .field(field)
        .with_context(|| format!("missing GitHub event field: {field}"))
}

fn is_team_member(login: &str) -> Result<bool> {
    let team_members =
        fs::read_to_string(".github/TEAM_MEMBERS").context("read .github/TEAM_MEMBERS")?;
    Ok(team_members.lines().any(|line| line == login))
}

fn title_is_exempt(title: &str) -> bool {
    matches_title_prefix(title, &["docs", "refactor", "feat"])
}

fn matches_title_prefix(title: &str, allowed: &[&str]) -> bool {
    let lower = title.trim_start();
    allowed.iter().any(|prefix| {
        let base = format!("{prefix}:");
        if lower.starts_with(&base) {
            return true;
        }
        if lower.starts_with(prefix) {
            let Some(rest) = lower.strip_prefix(prefix) else {
                return false;
            };
            if let Some(rest) = rest.strip_prefix('(') {
                if let Some(end) = rest.find(')') {
                    let scope = &rest[..end];
                    return !scope.is_empty()
                        && scope.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                        && rest[end + 1..].trim_start().starts_with(':');
                }
            }
        }
        false
    })
}

fn has_section(body: &str, heading: &str) -> bool {
    extract_section(body, heading).is_some()
}

fn extract_section(body: &str, heading: &str) -> Option<String> {
    let mut in_section = false;
    let mut lines = Vec::new();
    for line in body.lines() {
        if !in_section {
            if line.trim_end() == heading {
                in_section = true;
            }
            continue;
        }
        if line.starts_with("### ") {
            break;
        }
        lines.push(line);
    }
    if in_section {
        Some(lines.join("\n"))
    } else {
        None
    }
}

fn contains_issue_reference(section: &str) -> bool {
    let lower = section.to_lowercase();
    if lower.contains("closes #") || lower.contains("fixes #") || lower.contains("resolves #") {
        return true;
    }

    let mut chars = section.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '#' && chars.peek().is_some_and(|next| next.is_ascii_digit()) {
            return true;
        }
    }
    false
}

fn gh_api<const N: usize>(args: [&str; N]) -> Result<()> {
    let output = ProcessCommand::new("gh")
        .args(["api"])
        .args(args)
        .output()
        .context("running gh api")?;
    if !output.status.success() {
        bail!("gh api {:?} failed with status {}", args, output.status);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_sections() {
        let body = "### What does this PR do?\nhello\n### Type of change\n- [x] feat";
        assert_eq!(
            extract_section(body, "### What does this PR do?").as_deref(),
            Some("hello")
        );
        assert_eq!(
            extract_section(body, "### Type of change").as_deref(),
            Some("- [x] feat")
        );
    }

    #[test]
    fn detects_issue_references() {
        assert!(contains_issue_reference("Closes #123"));
        assert!(contains_issue_reference("Fixes #123"));
        assert!(contains_issue_reference("see #456"));
        assert!(!contains_issue_reference("no issue"));
    }

    #[test]
    fn parses_rfc3339_timestamps() {
        let parsed = parse_timestamp("2026-01-01T12:34:56Z").unwrap();
        assert_eq!(parsed.to_rfc3339(), "2026-01-01T12:34:56+00:00");
    }
}
