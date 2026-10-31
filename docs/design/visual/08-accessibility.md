# 08 — Accessibility

The portal should work for people with low vision, colour blindness, or who use a
keyboard or screen reader. Two parts: the `accessible` built-in theme, and requirements
that hold for **every** theme, because a high-contrast palette does not help a
screen-reader user if the buttons have no names.

Target: WCAG 2.2 level AA for every theme, AAA contrast for `accessible`.

## The `accessible` theme

Defined in [`02`](02-builtin-themes.md#accessible). What each choice is for:

| Choice | For | WCAG |
|---|---|---|
| 7:1 text contrast, light (navy on white) and dark (white and yellow on black) | low vision; dark suits photophobia and many cataract users | 1.4.6 Contrast (Enhanced) |
| Atkinson Hyperlegible Next | low vision: the Braille Institute designed it so easily confused letters (`I l 1`, `O 0`, `b d`) differ | — |
| text scale 1.25, letter spacing 0.2 | low vision, dyslexia | 1.4.12 Text Spacing |
| 2px solid borders, no grey-fill separation | low contrast sensitivity | 1.4.11 Non-text Contrast |
| 3px focus ring in the `focus` colour | keyboard users, low vision | 2.4.7 Focus Visible, 2.4.13 Focus Appearance |
| 48px minimum targets | tremor, low dexterity, touch | 2.5.8 Target Size (Minimum) asks 24; 48 is the Material guideline |
| `reduce_motion` | vestibular disorders | 2.3.3 Animation from Interactions |
| `status_icons` | colour blindness | 1.4.1 Use of Color |

Picking `accessible` is one click in the theme picker; the picker itself must be reachable
by keyboard and screen reader in every theme, or the people who need it most cannot get
to it.

## Requirements for every theme

These are theme-independent portal changes, landing in increment 2.

### 1. A semantics tree from the start

Flutter web draws to a canvas and only builds its accessibility DOM after the user
presses a hidden "Enable accessibility" button. In the Playwright session that found the
CORS bug, the nav rail exposed **no accessible names**: a screen reader sees an empty
canvas until it finds that button.

Fix: call `SemanticsBinding.instance.ensureSemantics()` at start-up, so the tree is
always there. Measure the cost on the Fleet page with many drafts; if it is large, keep
it on and make the tree cheaper, rather than turning it back off.

### 2. Every control has a name

- Nav rail destinations already have visible labels (`labelType: all`); check they
  appear in the semantics tree as `Launch`, `Prompts`, … with the selected one marked.
- Every `IconButton` has a `tooltip` (Flutter uses it as the semantics label). Today 21
  of 22 do; the exception is the remove-edge button in the Graph editor
  (`graph_page.dart:523`). Other tappables (`InkWell`, `GestureDetector`, cards) get an
  explicit `Semantics(label:, button: true)`.
- Graph nodes and edges get labels ("node: plan, 3 outgoing edges"), since the graph is
  otherwise a picture.
- The theme picker cards are buttons named after the theme, with the selected one marked
  `selected`.

The widget test `positive_labeled_targets` in [`07`](07-testing.md#accessibility-tests-dart-widget-tests)
enforces this for every page.

### 3. Keyboard only

- Tab and Shift+Tab reach every control in reading order; arrow keys move within the nav
  rail; Enter and Space activate.
- A visible focus ring, in the theme's `focus` colour at `focus_ring_width`, on every
  focusable widget. Material's default focus highlight is a faint overlay; the mapper
  replaces it with a solid outline.
- No keyboard traps: dialogs return focus to what opened them; Escape closes them.
- A "Skip to content" action as the first focus stop, so keyboard users do not tab
  through the nine nav items on every page.

### 4. Respect the user's settings

| Setting | Source | Portal behaviour |
|---|---|---|
| Text size | browser zoom / OS text scale → `MediaQuery.textScaler` | Multiplied with the theme's `scale`; layouts reflow without clipping up to 200% (WCAG 1.4.4) |
| Reduced motion | `prefers-reduced-motion` → `MediaQuery.disableAnimations` | Page transitions and spinners become instant or static, whatever the theme says |
| Dark mode | `prefers-color-scheme` | Used when the user's mode is `SYSTEM` ([`04`](04-per-user-active-theme.md)) |
| High contrast | `prefers-contrast: more` / `forced-colors` | If the user has no saved preference, default to `accessible` |

### 5. Never colour alone

Every place colour carries meaning also has an icon or text:

| Where | Today | Becomes |
|---|---|---|
| Review status (Fleet) | coloured text | icon + label: ✓ approved, ✎ draft, ↑ posted, ⚠ failed |
| Graph edge kinds | colour only | colour + line style (solid, dashed, dotted) + a legend |
| Health dots (Router, Launch) | green/red dot | dot + "healthy" / "down" text |
| Form errors | red border | red border + error text + an error icon |

### 6. Contrast is a gate

`validate_theme` refuses a theme below its declared level ([`06`](06-security-and-validation.md#contrast)),
and `portal-visual` runs `textContrastGuideline` against each built-in, so neither a
theme nor a hardcoded colour can slip through.

## How it is checked

| Check | Where | Gate? |
|---|---|---|
| contrast pairs per theme | `validate_theme` (Rust + Dart) | yes |
| `textContrastGuideline`, `labeledTapTargetGuideline`, `androidTapTargetGuideline` per theme × page | `portal-widget` / `portal-visual` | yes |
| 200% text scale overflow | widget test `boundary_text_scale_2x` | yes |
| a11y tree snapshot, keyboard walk, 200% zoom, reduced motion in a real browser | Playwright review ([`09`](09-visual-review.md)) | human review before merge |
| a screen reader (Orca on l2) on Launch and Fleet | manual, once per UI increment | human review |

Automated checks find missing names and low contrast, not whether the result is usable.
The Orca pass is the check that a person can actually get from sign-in to reading a
draft review without seeing the screen.
