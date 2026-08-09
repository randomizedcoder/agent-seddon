# Example cognition graphs

Graded, runnable cognition-graph documents
([design](../../docs/design/cognition-graph/04-graph-config.md)) — each is a
working scenario file **and** an integration-test fixture
(`crates/agent-graph/tests/examples.rs` asserts every file here loads,
validates clean, and parses to its deterministic `testdata` twin):

| File | Flow | Needs |
|---|---|---|
| [`simple.textproto`](simple.textproto) | consensus gate only (`generate → critic_gate`) | a provider named `glm` |
| [`intermediate.textproto`](intermediate.textproto) | gate + background summary/facts distillation + instant compaction — the full increments 01–03 behavior as a document | `glm` + `[digest] store` |
| [`advanced.textproto`](advanced.textproto) | the full flow + a fork: safety-lens branch (with its own gate loop) × performance-lens branch, `all` join, synthesize merge (losers → the alternatives ledger), final gate, background distillation, instant compaction | `glm` + `[digest] store` |
| [`economical.textproto`](economical.textproto) | role routing: the expensive gate pair answers, while summary/facts/objective route to a cheap `local` provider (node `provider` params + a capability edge) — same cognition, a fraction of the bill | `glm` + `local` + `[digest] store` |

Run one with:

```sh
agent --cognition-graph config/cognition/intermediate.textproto "<goal>"
```

(equivalent to `[graph] store = "file", file = …`). Documents reference
providers/stores **by name** — endpoints, keys, and connection details stay in
`config/agent.toml`. An invalid document is rejected wholesale at startup with
its typed per-node issues.
