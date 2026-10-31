# 09 — Visual review with Playwright

Goldens catch pixel changes, and the contrast and target guidelines catch measurable
problems. Neither tells you whether TN3270 looks like a 3270, or whether a page is
pleasant to read. So every increment that changes the UI gets a review in a real browser,
driven by the Playwright MCP, before it is offered for merge.

This is a human step that Claude runs and the user signs off on; it is not a
`nix flake check` gate.

## 1. Serve a fresh bundle

- Before merge: serve the branch's portal with `nix run .#portal-web` (it builds the
  bundle with `flutter build web` and serves it), pointed at l2's gRPC-web edge, or
  deploy the branch to l2 (`nix flake update agent-seddon` against the branch, rebuild).
- After merge: the live portal at `http://l2:8092` once l2 is rebuilt.
- Clear the browser cache first with CDP `Network.clearBrowserCache`. The static server
  sends a one-year cache header, and a stale bundle has already produced a false
  `UNIMPLEMENTED` once.

## 2. Shoot the matrix

For each combination, select the theme, open the tab, wait for the data to load, and
take a screenshot into the scratchpad:

| Axis | Values |
|---|---|
| Theme | prototype, tn3270, windows, macos, accessible (+ any theme the increment adds) |
| Mode | light, dark (tn3270 is dark only) |
| Viewport | 1400×1000 (desktop), 390×844 (phone) |
| Tab | Launch, Prompts, Graph, Agent, Router, Fleet (with Sessions open and a draft selected), Forges, Access, Settings |

That is about 160 screenshots. Navigate by semantics label (`getByRole('button', { name:
'Fleet' })`) once [`08`](08-accessibility.md#1-a-semantics-tree-from-the-start) lands;
until then the canvas needs coordinates.

## 3. Accessibility pass

In the same browser session:

| Step | How | Pass when |
|---|---|---|
| a11y tree | `browser_snapshot` on each tab | every nav item, button and field has a name; the selected tab is marked |
| keyboard walk | `Tab`/`Shift+Tab`/`Enter` only: sign in, open Fleet, open a draft, switch theme | every step reachable, focus ring always visible, no trap |
| 200% zoom | `document.body.style.zoom` or a 2× device scale, `accessible` theme | no clipped or overlapping text, no horizontal scroll on phone width |
| reduced motion | `emulateMedia({ reducedMotion: 'reduce' })` | page changes and spinners are static |
| forced high contrast | `emulateMedia({ forcedColors: 'active' })`, no saved preference | portal starts in `accessible` |
| console | `browser_console_messages` | no errors or overflow warnings |

## 4. Look at every screenshot

Claude reads every image and notes, per theme and tab:

- legibility: font size, weight, line length, the body font actually loaded (not a
  fallback);
- clipping, overflow, text cut off, controls overlapping on phone width;
- contrast that the gates cannot see (text over images, disabled states, hover);
- character: TN3270 reads as a green screen, Windows as Fluent, macOS as macOS;
- leftovers: any colour still coming from the old hardcoded palette.

Each finding is fixed on the branch and the affected screenshots are retaken.

## 5. Contact sheet for the user

Claude publishes a private Artifact: a grid per theme (rows = tabs, columns = light and
dark, desktop and phone), the accessibility results table, and the list of findings
with what was fixed. The user gets the link and reviews it before the PR is offered for
merge. Screenshots stay in the scratchpad and are not committed; goldens are the
committed record.
