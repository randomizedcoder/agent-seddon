# Skills

A **skill** is a reusable instruction snippet the agent can pull into a
conversation on demand. Each skill is a `SKILL.md` file with YAML-ish frontmatter
and a markdown body:

```markdown
---
name: changelog
description: Write a clear changelog entry from a diff or commit range
---
# Writing a changelog entry
…instructions…
```

## Layout

Skills are discovered (in order) from `skills/` and `.agent/skills/`, either as:

- `skills/<name>/SKILL.md` (a directory per skill — the Agent Skills convention), or
- `skills/<name>.md` (a single flat file).

## Using them (REPL)

- `/skills` — list discovered skills (name — description).
- `/skill:<name>` — load that skill's body into the conversation; it applies to
  your next message (progressive disclosure — only the chosen skill's body enters
  context, not every skill's).

See [`changelog/SKILL.md`](changelog/SKILL.md) for an example.
