# context.d — user context files

Drop small markdown files here to add fixed entries to the model's context on
every run (project rules, persona, coding conventions, output format, reminders).
Unlike semantic memory (`.agent/memory/*.md`, which is keyword-recalled), these are
**always** injected.

## Layout

```
context.d/
├── prepend/   # injected BEFORE the conversation (folded into the system prompt)
│   ├── 0001_persona.md
│   └── 0020_project_rules.md
└── append/    # injected AFTER the goal (a trailing system message)
    └── 0010_output_format.md
```

- File names start with a number (`NNNN_`) that orders them **ascending**.
- Only `*.md` files are read. Files without a numeric prefix sort last.
- This `README.md` lives at the top level and is **not** injected — only files
  inside `prepend/` and `append/` are loaded.

The directory is configurable via `[context_files] dir` in `config/agent.toml`
(default `context.d`). A missing directory simply means no injection.
