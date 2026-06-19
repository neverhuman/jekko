#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

repos=(
  jekko-core
  jekko-mcp
  jekko-deploy
  jekko-jnoccio
  jekko-jailgun
  jekko-zyal
  jekko-search
  jekko-memory
  jekko-agent
)

declare -A role=(
  [jekko-core]=core
  [jekko-mcp]=mcp
  [jekko-deploy]=deploy
  [jekko-jnoccio]=router
  [jekko-jailgun]=web
  [jekko-zyal]=domain
  [jekko-search]=shared
  [jekko-memory]=data
  [jekko-agent]=agent
)

declare -A profile=(
  [jekko-core]=rust-core
  [jekko-mcp]=rust-mcp
  [jekko-deploy]=ops
  [jekko-jnoccio]=rust-router
  [jekko-jailgun]=rust-web
  [jekko-zyal]=rust-domain
  [jekko-search]=rust-shared
  [jekko-memory]=rust-data
  [jekko-agent]=rust-agent
)

repo_exists_remote() {
  local repo="$1"
  jeryu forge repo list --json | jq -e --arg name "$repo" '.[] | select(.name == $name)' >/dev/null
}

create_remote() {
  local repo="$1"
  if ! repo_exists_remote "$repo"; then
    printf 'creating remote %s\n' "$repo"
    jeryu forge repo create "$repo" --owner jeryu --default-branch main >/dev/null
  fi
}

ensure_gitd_repo() {
  local repo="$1"
  local gitd_path="/home/ubuntu/.local/share/jeryu/git/jeryu/${repo}.git"
  if [ ! -d "$gitd_path" ]; then
    mkdir -p "$(dirname "$gitd_path")"
    git init --bare --initial-branch=main "$gitd_path" >/dev/null
  fi
}

write_common_files() {
  local repo="$1"
  local repo_role="$2"
  local repo_profile="$3"

  mkdir -p src ops/ci .github/workflows agent

  cat > README.md <<EOF
# ${repo}

${repo} is the ${repo_role} split-family repository in the Jekko baseline.
It is a standalone checkout with its own CI, Jankurai metadata, and forge
remotes.
EOF

  cat > AGENTS.md <<EOF
# ${repo} Agent Instructions

- Read \`agent/JANKURAI_STANDARD.md\` first.
- Keep the repo-root lanes thin and shell-driven under \`ops/ci/*.sh\`.
- Keep the remotes wired to the canonical Jeryu and GitHub URLs.
- Pin Jankurai to \`1.6.1\` and keep the onboarding gate explicit.
EOF

  cat > Cargo.toml <<EOF
[package]
name = "${repo}"
version = "0.1.0"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"

[workspace]
EOF

  cat > src/lib.rs <<EOF
/// Canonical identity for the ${repo} split-family checkout.
pub const REPOSITORY: &str = "${repo}";

/// Role recorded in the split-family manifest.
pub const ROLE: &str = "${repo_role}";

/// Profile recorded in the split-family manifest.
pub const PROFILE: &str = "${repo_profile}";

/// Return the repo identity tuple used by the smoke tests.
pub fn identity() -> (&'static str, &'static str, &'static str) {
    (REPOSITORY, ROLE, PROFILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_stable() {
        assert_eq!(identity(), (REPOSITORY, ROLE, PROFILE));
    }
}
EOF

  cat > Justfile <<'EOF'
set shell := ["bash", "-euo", "pipefail", "-c"]

fast:
	bash ops/ci/fast.sh

check:
	bash ops/ci/check.sh

test:
	bash ops/ci/test.sh

typecheck:
	bash ops/ci/typecheck.sh

build:
	bash ops/ci/build.sh
EOF

  cat > .gitignore <<'EOF'
/target
/.jankurai
/agent/repo-score.json
/agent/repo-score.md
EOF

  cat > ops/ci/fast.sh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
export PATH="${HOME}/.local/bin:${HOME}/.cargo/bin:${PATH}"
cargo fmt --all -- --check
cargo test --locked --lib
EOF

  cat > ops/ci/check.sh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
cargo check --locked --all-targets
EOF

  cat > ops/ci/test.sh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
cargo test --locked --all-targets
EOF

  cat > ops/ci/typecheck.sh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
cargo clippy --locked --all-targets --all-features -- -D warnings
EOF

  cat > ops/ci/build.sh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
cargo build --locked --all-targets
EOF

  cat > ops/ci/jankurai.sh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

JANKURAI_VERSION="1.6.1"
JANKURAI_REV="c7360a88b1e1869626df0450f1e28221047832db"

if ! command -v jankurai >/dev/null 2>&1; then
  cargo install --root "${HOME}/.local" --git https://github.com/neverhuman/jankurai --rev "${JANKURAI_REV}" --locked jankurai
  export PATH="${HOME}/.local/bin:${PATH}"
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required for the strict audit gate" >&2
  exit 1
fi

mkdir -p agent
jankurai audit . --mode advisory --json agent/repo-score.json --md agent/repo-score.md
jq -e --arg version "${JANKURAI_VERSION}" '.auditor_version == $version' agent/repo-score.json >/dev/null

blockers="$(jq '(.conformance_blockers // []) | length' agent/repo-score.json)"
hard_findings="$(jq '(.hard_findings // .decision.hard_findings // ([.findings[]? | select(.hardness == "hard" or .severity == "high" or .severity == "critical")] | length))' agent/repo-score.json)"
caps="$(jq '(.caps_applied // []) | length' agent/repo-score.json)"
score="$(jq '(.score // 0)' agent/repo-score.json)"
minimum="$(jq '(.minimum_score // .decision.minimum_score // 0)' agent/repo-score.json)"
printf 'jankurai strict gate: score=%s minimum=%s blockers=%s hard_findings=%s caps=%s\n' "$score" "$minimum" "$blockers" "$hard_findings" "$caps"
if [ "$blockers" -ne 0 ] || [ "$hard_findings" -ne 0 ] || [ "$caps" -ne 0 ]; then
  exit 1
fi
EOF

  chmod +x ops/ci/*.sh

  cat > .github/workflows/ci.yml <<EOF
name: ci

on:
  workflow_dispatch:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

jobs:
  fast:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683 # v4.2.2
      - uses: dtolnay/rust-toolchain@29eef336d9b2848a0b548edc03f92a220660cdb8 # stable
      - run: bash ops/ci/fast.sh
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683 # v4.2.2
      - uses: dtolnay/rust-toolchain@29eef336d9b2848a0b548edc03f92a220660cdb8 # stable
      - run: bash ops/ci/check.sh
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683 # v4.2.2
      - uses: dtolnay/rust-toolchain@29eef336d9b2848a0b548edc03f92a220660cdb8 # stable
      - run: bash ops/ci/test.sh
  typecheck:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683 # v4.2.2
      - uses: dtolnay/rust-toolchain@29eef336d9b2848a0b548edc03f92a220660cdb8 # stable
      - run: bash ops/ci/typecheck.sh
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683 # v4.2.2
      - uses: dtolnay/rust-toolchain@29eef336d9b2848a0b548edc03f92a220660cdb8 # stable
      - run: bash ops/ci/build.sh
EOF

  cat > .github/workflows/jankurai.yml <<EOF
name: jankurai

on:
  workflow_dispatch:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

jobs:
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683 # v4.2.2
      - uses: dtolnay/rust-toolchain@29eef336d9b2848a0b548edc03f92a220660cdb8 # stable
      - run: bash ops/ci/jankurai.sh
EOF

  cat > .gitlab-ci.yml <<EOF
image: rust:1.82

stages:
  - fast
  - check
  - test
  - typecheck
  - build
  - jankurai

fast:
  stage: fast
  script:
    - bash ops/ci/fast.sh

check:
  stage: check
  script:
    - bash ops/ci/check.sh

test:
  stage: test
  script:
    - bash ops/ci/test.sh

typecheck:
  stage: typecheck
  script:
    - bash ops/ci/typecheck.sh

build:
  stage: build
  script:
    - bash ops/ci/build.sh

jankurai:
  stage: jankurai
  script:
    - bash ops/ci/jankurai.sh
EOF

  cat > agent/JANKURAI_STANDARD.md <<EOF
# jankurai Standard Agent Bootstrap

Standard version: \`0.9.0\`

Read \`AGENTS.md\` first. This repo pins Jankurai to \`1.6.1\`.
EOF

  cat > agent/standard-version.toml <<EOF
auditor_version = "1.6.1"
paper_edition = "2026.05-ed8"
published = "2026-06-11"
release_tag = "v1.6.1"
schema_version = "1.9.0"
standard = "jankurai"
standard_version = "0.9.0"
EOF

  cat > agent/owner-map.json <<EOF
{
  "owners": {
    "AGENTS.md": "agent",
    "Cargo.toml": "workspace",
    "Justfile": "workspace",
    "README.md": "workspace",
    ".github/": "ops",
    ".gitlab-ci.yml": "ops",
    ".gitignore": "agent",
    "agent/": "agent",
    "ops/": "ops",
    "src/": "workspace"
  },
  "workspace": "${repo}"
}
EOF

  cat > agent/test-map.json <<EOF
{
  "tests": {
    "AGENTS.md": {
      "command": "just fast",
      "purpose": "verify the repo instructions stay visible"
    },
    "Cargo.toml": {
      "command": "just fast",
      "purpose": "verify the package manifest stays parseable"
    },
    "Justfile": {
      "command": "just fast",
      "purpose": "verify the lane wrapper stays runnable"
    },
    "README.md": {
      "command": "just fast",
      "purpose": "verify the repo README stays in sync"
    },
    ".github/workflows/": {
      "command": "just fast",
      "purpose": "verify workflow dispatch stays script-only"
    },
    ".gitlab-ci.yml": {
      "command": "just fast",
      "purpose": "verify the GitLab pipeline stays dispatch-only"
    },
    "agent/": {
      "command": "just fast",
      "purpose": "verify the Jankurai metadata bundle stays parseable"
    },
    "ops/ci/": {
      "command": "just fast",
      "purpose": "verify the shell lane scripts stay runnable"
    },
    "src/": {
      "command": "just test",
      "purpose": "verify the crate body and smoke test stay green"
    }
  }
}
EOF

  cat > agent/generated-zones.toml <<EOF
# Generated zone manifest for Jekko control-plane outputs.
# DO NOT EDIT BY HAND.
# Source: scripts/bootstrap-split-family.sh
# Regenerate: \`bash scripts/bootstrap-split-family.sh\`

[[zone]]
command = "cargo run -p zyalc --manifest-path crates/zyalc/Cargo.toml -- compile agent/zyal/sandbox-lanes.zyal --out generated/sandbox-lanes.toml"
path = "generated/sandbox-lanes.toml"
read_only = true
source = "zyalc"

[[zone]]
command = "cargo run -p zyalc --manifest-path crates/zyalc/Cargo.toml -- compile agent/zyal/ambitious-superworkflow.zyal --out generated/superworkflows/ambitious-superworkflow.superworkflow.json"
path = "generated/superworkflows/ambitious-superworkflow.superworkflow.json"
read_only = true
source = "zyalc"

[[zone]]
command = "bash ops/ci/jankurai.sh"
path = "agent/repo-score.json"
read_only = false
source = "installed jankurai binary"

[[zone]]
command = "bash ops/ci/jankurai.sh"
path = "agent/repo-score.md"
read_only = false
source = "installed jankurai binary"

[[zone]]
command = "cargo run -p zyalc --manifest-path crates/zyalc/Cargo.toml -- schema --out generated/ir"
path = "generated/ir/flowgraph.schema.json"
read_only = true
source = "contracts/node-types.json"

[[zone]]
command = "cargo run -p zyalc --manifest-path crates/zyalc/Cargo.toml -- schema --out generated/ir"
path = "generated/ir/zyal.d.ts"
read_only = true
source = "contracts/node-types.json"
EOF

  cargo generate-lockfile >/dev/null

  git add \
    .github/workflows/ci.yml \
    .github/workflows/jankurai.yml \
    .gitignore \
    .gitlab-ci.yml \
    AGENTS.md \
    Cargo.lock \
    Cargo.toml \
    Justfile \
    README.md \
    agent/JANKURAI_STANDARD.md \
    agent/generated-zones.toml \
    agent/owner-map.json \
    agent/standard-version.toml \
    agent/test-map.json \
    ops/ci/build.sh \
    ops/ci/check.sh \
    ops/ci/fast.sh \
    ops/ci/jankurai.sh \
    ops/ci/test.sh \
    ops/ci/typecheck.sh \
    src/lib.rs
  git commit -m "Bootstrap split-family baseline" >/dev/null
}

for repo in "${repos[@]}"; do
  ensure_gitd_repo "$repo"
  create_remote "$repo"
  if [ -d "$repo/.git" ]; then
    printf '%s already exists locally, skipping\n' "$repo"
    continue
  fi
  printf 'initializing %s\n' "$repo"
  mkdir -p "$repo"
  git -C "$repo" init -b main >/dev/null
  cd "$repo"
  write_common_files "$repo" "${role[$repo]}" "${profile[$repo]}"
  git remote add origin "http://127.0.0.1:8787/git/jeryu/${repo}.git"
  git remote add jeryu "http://127.0.0.1:8787/git/jeryu/${repo}.git"
  git remote add github "https://github.com/neverhuman/${repo}.git"
  git push -u origin main >/dev/null
  cd "$ROOT"
done
