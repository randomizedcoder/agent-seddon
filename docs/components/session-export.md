# Session export

Render a saved session transcript to a shareable artifact. Parity spec
[20](../parity/20-session-export.md).

After a long session you want to hand someone a readable artifact — HTML for a
bug report, markdown for a PR description, JSON for tooling. Two properties make
that safe and useful.

## Deterministic

The render is a **pure function of the transcript**. No wall clock, no random
ids, no map-iteration order reaches the output, so the same session exports to
the same bytes every time.

That is not cosmetic — it is what makes the output diffable, cacheable, golden-
testable, and what makes the instruction-count bench meaningful (a stable render
has a stable instruction count, so a bench move means the render actually
changed).

The non-obvious source of nondeterminism is tool-call arguments: they are
`serde_json::Value`, which is map-backed, so key order is not guaranteed. The
renderer sorts keys explicitly rather than hoping.

## Safe to share

**Redaction is on by default.** A transcript is exactly the artifact people paste
into bug reports, so leaking is the costlier default. Secrets are replaced with a
stable `[redacted:<rule>]` marker before any renderer sees the text — so all three
formats inherit it.

Redaction uses the [`Scanner`](scanner.md) seam when one is wired — this is the
first consumer of `Finding.span` — and a built-in fallback matcher otherwise, so
export is safe even in a build without the scanner. Redaction is itself
deterministic: findings are sorted before application, so rule-firing order can't
change the output.

Tool **arguments** are redacted too. They routinely carry file bodies and command
lines, which is where a credential most often ends up. Because redacting breaks
JSON validity, a redacted argument object is carried as a string rather than
emitted as malformed JSON.

Spans come from a scanner and are treated as data: out-of-range, inverted, and
mid-character spans are skipped rather than panicking, and overlapping findings
are dropped rather than interleaving into garbage.

## HTML safety

The transcript is fully attacker-influenced — a page the agent fetched, a file it
read, a model completion — and the export is a document someone opens in a
browser. Every interpolated value is escaped (`& < > " '`), including the session
id, which reaches both `<title>` and a `<code>` block. C0 control characters are
stripped, since they can smuggle terminal escape sequences into a rendered page.

> The security property is that no payload character can **start markup or break
> out of an attribute** — not that a dangerous-looking substring disappears. An
> escaped `&lt;img src=x onerror=…` still contains the text `onerror=`, inertly,
> and that is correct. The tests assert the property, not the substring.

The page is **self-contained**: CSS is inlined and there are no `<link>`,
`<script>`, `src=`, or `http(s)://` references, so it renders offline and phones
nobody. A test asserts that.

## Usage

```jsonc
{
  "session": "2026-07-21-1a2b",   // session id (no separators or `..`)
  "format": "html",               // md (default) | json | html
  "path": "reports/session.html", // relative to the working directory
  "redact": true                  // default true
}
```

Both the session id and the output path are model-supplied, so both are confined:
the id must not contain separators, `..`, NUL, or a leading `-` (it becomes a path
segment), and the output path goes through the shared canonicalizing `confine`.

## Formats

| Format | Shape |
|---|---|
| `md` | Readable digest: a heading per turn, tool calls as fenced JSON |
| `json` | Structured transcript for tooling — parseable, key-sorted |
| `html` | Self-contained themed page, light and dark |

Media blocks are **described, not dropped** (`[image: image/png, 20164 bytes]`) —
a transcript that silently omits an image misrepresents what happened.

## Deferred: cross-session recall

Spec 20 is explicitly two capabilities. This ships the **export** half; the
**cross-session search** half — a full-text index over past sessions so "how did
we fix the tantivy bug?" resolves to the session where it happened — is not
implemented.

It is a genuine piece of work rather than a small follow-on: `SearchBackend`'s
`reindex` walks a filesystem tree (`Manifest::scan`), so indexing the session
corpus means either a second tantivy backend rooted at the sessions directory or
a document-source abstraction in `agent-search`. The intended path is the former
(sessions are already one file per session), with the recall tool mapping an
index hit's path back to a session id.
