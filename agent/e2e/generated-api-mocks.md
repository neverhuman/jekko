# Generated API Mocks

Jekko has no browser web surface. Rendered UX proof uses the TUI-backed
tuiwright harness, and generated API mock coverage is represented by the
checked-in Jnoccio model catalog fixture at `jnoccio-fusion/config/models.json`.

The catalog is generated mock input for provider/model API states used by the
TUI dashboard and routing tests: loading, empty, error, success, and
permission-denied.

This is the TUI equivalent of an MSW/mock service worker fixture for rendered
state coverage; Jekko does not ship a browser service worker because the
primary rendered surface is terminal-backed.
