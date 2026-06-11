# `scripted_agent`

`scripted_agent` is a local deterministic provider for agent workflow tests. It implements the normal provider streaming interface, but it never performs network I/O, never reads API keys, and reports zero token usage in its built-in scenarios.

## Selecting it

Use provider `scripted_agent`. If no model is supplied, the runtime recommendation is `basic`.

Useful model/scenario ids:

- `basic` — deterministic text-only assistant response.
- `tool-read` — emits a streamed `read` tool call first, then a final response after the tool result is present in the next turn.
- `failure` — emits a deterministic provider error for error-path tests.

The adapter also accepts an explicit scenario override through provider options when tests construct `ProviderRequest` directly:

```json
{
  "scripted_agent": { "scenario_id": "tool-read" }
}
```

For runtime/CLI tests where provider options are not exposed, choose the scenario by setting the model id to one of the scenario ids above.

## Adding scenarios

Scenarios live as strict JSON fixtures beside the adapter in `crates/jekko-provider/src/providers/scripted_agent/`. Each fixture declares:

- stable `id`, `title`, `tags`, `provider`, and `model` metadata;
- ordered stages (`initial`, optionally `after-tool-result`);
- ordered frames such as `stream-start`, `text-delta`, `tool-call`, `usage`, `metadata`, `stream-end`, or `error`.

Fixtures are parsed with unknown fields rejected, duplicate ids rejected, and blank/empty scenarios rejected. Text and JSON string fields may use `{{last_user_text}}` and `{{first_path}}` for deterministic input-aware output.

## Test coverage

Provider tests validate fixture parsing, deterministic text output, tool-call path templating, and failure frames. Runtime tests cover provider/model selection and a no-credential `run_oneshot` call through the real provider executor.
