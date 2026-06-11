# scripted_agent

`scripted_agent` is Jekko's deterministic local provider for agent workflow tests. It implements the normal provider streaming contract without using API keys, token spend, sleeps, wall-clock output, or network access.

## Selecting it

Use provider id `scripted_agent`. The recommended model is `basic`, and each model id also names a scenario:

| Scenario/model | Purpose |
| --- | --- |
| `basic` | Streams a deterministic text response that echoes the last user request. |
| `tool-read` | Emits a `read` tool call for the first absolute path in the prompt, then returns a deterministic final response after the tool result. |
| `failure` | Emits a scripted provider error for failure and retry-path tests. |

For one-shot runtime calls, pass `provider = "scripted_agent"` and `model = "basic"`, `"tool-read"`, or `"failure"`. Adapter-level tests may also select a scenario with provider options: `scenario`, `scenario_id`, `scripted_agent_scenario_id`, or `{ "scripted_agent": { "scenario_id": "..." } }`.

## Adding scenarios

Scenarios live under `crates/jekko-provider/src/providers/scripted_agent/*.json` and are embedded with `include_str!` so tests do not depend on the current working directory. Fixture parsing is strict (`deny_unknown_fields`) and validates that each scenario has:

- non-blank `id` and `title`
- `provider = "scripted_agent"`
- at least one stage
- an `initial` stage
- non-empty frame lists

Supported frame types are `stream-start`, `text-delta`, `reasoning-delta`, `tool-call`, `usage`, `metadata`, `stream-end`, and `error`. Text and JSON string values may use `{{last_user_text}}` and `{{first_path}}`; expansion is deterministic and does not inspect the filesystem.

## Tests

`crates/jekko-provider/src/providers/scripted_agent.rs` covers strict fixture loading, deterministic text output, tool-call path expansion, and scripted failure emission.
