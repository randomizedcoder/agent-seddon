# hermes personality — attribution

The prompt in `0001_hermes.md` is extracted from Hermes Agent's prompt constants
(hermes assembles its prompt from Python string constants; this seed is the
`DEFAULT_AGENT_IDENTITY` plus the core stable-tier guidance blocks —
`MEMORY_GUIDANCE`, `SESSION_SEARCH_GUIDANCE`, `SKILLS_GUIDANCE` — copied verbatim).

- **Upstream:** https://github.com/NousResearch/hermes-agent
- **Source path:** `agent/prompt_builder.py` (`DEFAULT_AGENT_IDENTITY` + guidance constants)
- **Commit:** `d9ee342414042bba7bca43438f19d2fba9a54806`
- **License:** MIT — `Copyright (c) 2025 Nous Research`

MIT permits redistribution provided the copyright and permission notice are included;
they are aggregated in [`../LICENSES.md`](../LICENSES.md). This file is inert example
content — see [`../README.md`](../README.md).
