# 06 — Security and validation

A user theme is written by someone else and rendered in your session, so it is untrusted
input. The same rules as the rest of the repo apply ([CLAUDE.md](../../../CLAUDE.md)):
fail closed, cap everything, never sanitize into something valid.

## Where validation runs

| Place | What |
|---|---|
| Server, on `Put` and `SetActive` | Full validation; an invalid theme is never stored |
| Server, on read from the store | Validation again; a card that fails (an old version, a hand-edited file) is skipped and logged, not served |
| Portal, in `theme_mapper.dart` | The same rules again; a document that fails falls back to `prototype` and logs |
| Build | Every built-in validates, in Rust and Dart tests |

The Rust validator lives with the theme card (`validate()` on the `Card` trait, which
`agent-config-store` already calls on ingest and on read). The Dart mapper uses the same
rule table ([`07`](07-testing.md)), so both sides agree.

## Rules

| Field | Rule | Error |
|---|---|---|
| whole document | ≤ 64 KiB serialised | `TooLarge` |
| `id` | `agent_core::safe_segment`: ASCII letters, digits, `-`, `_`, `.` only; not `.`, `..` or a leading `-`; ≤ 64 chars | `BadId` |
| `name`, `author` | 1–64 chars; printable; no control or bidi-override characters (U+202A–U+202E, U+2066–U+2069) | `BadName` |
| `description` | ≤ 512 chars, same character rules | `BadName` |
| every colour | exactly `#RRGGBB` or `#RRGGBBAA`, hex digits only, ≤ 9 chars | `BadColor` |
| palettes | at least one of `light` / `dark`; without `seed`, every role is required | `Missing` |
| `nav_tint` keys | one of the nav tab ids; ≤ 16 entries | `UnknownKey` |
| `graph_edge` keys | one of the edge kinds; ≤ 16 entries | `UnknownKey` |
| font families | in the bundled allowlist ([`02`](02-builtin-themes.md#fonts)) or the generic `monospace` / `sans-serif` / `serif` | `UnknownFont` |
| `scale` | finite, 0.75–1.5 | `OutOfRange` |
| `letter_spacing` | finite, -1.0–2.0 | `OutOfRange` |
| weights | 100–900, multiple of 100 | `OutOfRange` |
| `corner_radius` | 0–32 | `OutOfRange` |
| `border_width` | 0–4 | `OutOfRange` |
| `focus_ring_width` | 1–6 | `OutOfRange` |
| `min_target_size` | 40–64 | `OutOfRange` |
| contrast | each text pair meets the declared level (below) | `Contrast { pair, ratio, required }` |

Numbers are **rejected**, not clamped: a theme with `scale: 9` is a mistake the author
should see, and silently drawing it at 1.5 hides that. (Clamping is for numbers the
server receives at runtime from a provider, where rejecting is not possible.)

## Contrast

Computed with the WCAG 2.2 relative-luminance formula. Alpha colours are composited over
their background first.

| Pair | AA | AAA |
|---|---|---|
| `on_surface` / `surface` and `background` | 4.5 | 7 |
| `on_surface_variant` / `surface_variant` | 4.5 | 7 |
| `on_primary` / `primary`, `on_secondary` / `secondary`, `on_error` / `error` | 4.5 | 7 |
| `error`, `success`, `warning`, `info`, `muted` / `surface` (shown as text) | 4.5 | 7 |
| `code_foreground` / `code_background` | 4.5 | 7 |
| `primary` / `surface` (links, icons) | 3 | 4.5 |
| `focus` / `background` | 3 | 3 |
| `outline` / `background` | — | 3 |

The error names the failing pair and both ratios, e.g.
`contrast: on_primary #FFFFFF on primary #007AFF is 4.02:1, needs 4.5:1 (AA)`, so an
author knows what to change.

## What a theme cannot do

No field is a URL, a path, a script, a CSS string, or a widget name. Fonts are chosen from
files already in the bundle, so a theme cannot make the browser fetch anything. The worst
a valid theme can do is look ugly, and the contrast gate limits how unreadable it can be.
