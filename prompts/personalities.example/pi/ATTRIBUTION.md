# pi personality — attribution

The prompt in `0001_pi.md` is extracted from the `buildSystemPrompt` template literal
in the pi project (pi assembles its prompt in code rather than a data file, so this is
the static literal with pi's default tool list and guidelines; `${…}` markers are pi's
runtime interpolations, preserved as-is).

- **Upstream:** https://github.com/earendil-works/pi (monorepo `badlogic/pi-mono`)
- **Source path:** `packages/coding-agent/src/core/system-prompt.ts` (`buildSystemPrompt`)
- **Commit:** `3da591ab74ab9ab407e72ed882600b2c851fae21`
- **License:** MIT — `Copyright (c) 2025 Mario Zechner`

MIT permits redistribution provided the copyright and permission notice are included;
they are aggregated in [`../LICENSES.md`](../LICENSES.md). This file is inert example
content — see [`../README.md`](../README.md).
