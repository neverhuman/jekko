# Multi-User Key Pool

`jnoccio-fusion` can route upstream calls across multiple per-user
credential bundles. Each user has their own provider keys, their own
budget enforcement hook, and their own routing slot — so a single
gateway can fan a workload across many parallel keys without the
operator manually merging them into one config file.

## On-disk layout

```text
~/.jekko/
  users/
    user/                # default user; always present after `jekko jnoccio init`
      llm.env            # provider keys (KEY=VALUE per line, dotenv format)
      state.sqlite       # per-user budget + usage state (created on first write)
    user_1/              # additional users; created via `jekko keys add --user user_1`
      llm.env
      state.sqlite
    user_2/
      llm.env
      state.sqlite
    .balancer.sqlite     # shared round-robin cursor (reuses jekko-runtime's existing table)
```

Each `llm.env` is a standard dotenv-style file. Only the provider env
names that match the gateway's resolved-model entries are honored;
unknown keys are ignored. Empty values count as missing.

## Environment variables

| Variable | Purpose |
|---|---|
| `JEKKO_HOME` | Override the root used by the key-pool scanner. Defaults to `~/.jekko`. Honored for test isolation and for non-home installs. The scanner reads `<JEKKO_HOME>/users/*/llm.env`. |
| `JNOCCIO_DEVELOPER_KEY` | Developer-unlock gate. Required to provision additional users beyond `user`. `jekko keys add --user user_1` fails closed with `creating extra users requires JNOCCIO_DEVELOPER_KEY developer unlock` if the unlock is absent. |
| `JNOCCIO_UPSTREAM_KEY_SOURCE` | Selects the gateway's credential-resolution mode. `config_env` (default) keeps the legacy single-process env-var path. `users_pool` switches to the multi-user fanout described below. |

`jekko jnoccio status` shows `developer_unlocked: bool` and
`home_env_has_developer_key: bool` so you can verify the unlock state
without echoing the secret.

## Gateway fanout (`users_pool` mode)

When `JNOCCIO_UPSTREAM_KEY_SOURCE=users_pool`,
`jnoccio_fusion::config::resolve_models` calls
`zyal_key_pool::KeyPool::scan(<JEKKO_HOME>/users/)` and produces one
`ResolvedModel` per `(config entry, matching user slot)` pair:

- `route_slot_id` = `"{provider}/{model}@{user_id}"`
- `credential_user_id` = the user folder name (e.g. `user_1`)
- `credential_env_name` = the provider env name the slot satisfied
- `api_key` = the value read from that user's `llm.env`

The fusion router uses `route_slot_id` (not `visible_id`) as its
routing key, so per-user slots compete independently in the balancer.
A config entry with no matching slot still emits a single
missing-key row so health diagnostics stay honest.

`ConfigEnv` mode collapses to `visible_id == route_slot_id`, which
preserves all legacy behavior bit-for-bit.

### Example

Two users, one model entry for `openai/gpt-4o-mini`:

```text
~/.jekko/users/user/llm.env       OPENAI_API_KEY=sk-aaa...
~/.jekko/users/user_1/llm.env     OPENAI_API_KEY=sk-bbb...
```

With `JNOCCIO_UPSTREAM_KEY_SOURCE=users_pool` the gateway resolves two
routing slots:

```text
openai/gpt-4o-mini@user     (uses sk-aaa...)
openai/gpt-4o-mini@user_1   (uses sk-bbb...)
```

Both are routable; the balancer round-robins between them. Their
counters at `/metrics` carry the same `{model="gpt-4o-mini",
provider="openai"}` labels so per-model dashboards stay aggregated,
but per-user accounting flows through the user slot's `state.sqlite`.

## PolicyHook gate

Every dispatch passes through a `zyal_key_pool::PolicyHook` before the
upstream call is made:

```rust
trait PolicyHook: Send + Sync {
    fn check_and_reserve(
        &self,
        user_id: &str,
        provider: &str,
        model: &str,
        estimated_tokens: u64,
    ) -> BudgetDecision;
}
```

`BudgetDecision::Refuse` fails the request as
`GatewayError::BudgetExceeded` (HTTP 429); `Allow` proceeds.

The default hook is `zyal_key_pool::AlwaysAllow`, a stub that always
returns `Allow`. It exists so the gate is structurally present
without forcing every dev workflow to wire real enforcement. An
`EnforceDailyCaps` implementation backed by the per-user
`state.sqlite` `user_budget` / `user_usage_day` tables is the
follow-up; the schema is already created idempotently by
`zyal_key_pool::budget::ensure_schema()`.

Token accounting passed to the hook is `0` today; Phase E adds real
estimation. Operators wiring `EnforceDailyCaps` early should treat the
estimate as a lower bound and prefer post-call true-up via
`record_usage` (also already in the budget module).
