# Provider cassettes (VCR replay)

These are the **recorded response bodies** replayed by
[`tests/vcr_matrix.rs`](../../vcr_matrix.rs) so the shipped binary's provider wire
paths — openai-compat `/chat/completions` **and** Anthropic `/messages` — are driven
end to end **offline**, in the hermetic `test` check.

Each file is an ordered JSON array of the provider's own response bodies for the
canonical two-turn "write hello.c" tool loop:

| Cassette | Wire | Turn 0 | Turn 1 |
|----------|------|--------|--------|
| `openai_compat.json` | OpenAI `/chat/completions` | `tool_calls` (write_file) | final text |
| `anthropic.json` | Anthropic `/messages` | `tool_use` block (write_file) | final text |

The `CassetteServer` in the test replays these in order over a loopback port; the
agent's `[provider] base_url` points at it. A **missing cassette is a hard failure**,
never a silent skip.

## Refreshing

The bodies are the provider's own wire shapes, so re-capture them from a real
endpoint when a wire format changes — never hand-edit escaping:

```sh
# openai-compat (e.g. a local ollama)
AGENT_VCR_PROVIDER=openai-compat \
AGENT_VCR_BASE_URL=http://localhost:11434/v1 AGENT_VCR_MODEL=llama3.1:latest \
AGENT_VCR_API_KEY=ollama  nix run .#vcr-record

# anthropic
AGENT_VCR_PROVIDER=anthropic \
AGENT_VCR_BASE_URL=https://api.anthropic.com/v1 AGENT_VCR_MODEL=claude-haiku-4-5 \
AGENT_VCR_API_KEY=sk-ant-...  nix run .#vcr-record
```

`vcr-record` writes **only response bodies** — never the request, API key, or any
header — so no secret is persisted. Review the diff before committing.
