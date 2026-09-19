# 02 — The test-spec table (single source of truth)

Every test — Layer A and Layer B — is a **row** in a per-page spec table. Adding a test
is adding a row; adding an element is adding its `Key` plus its rows. The tables below
are the design of record for what gets tested; the executable form is a Dart data
structure the runner iterates (mirroring the Rust `rstest` `#[case::<prefix>_…]`
convention in [`crates/agent-tools/src/edit.rs`](../../../crates/agent-tools/src/edit.rs)).

## Row schema

| field | meaning |
|---|---|
| `page` / `element_id` | page + the widget `Key` (the id scheme below) |
| `case` | `positive_ / negative_ / boundary_ / corner_ / adversarial_` + short name |
| `description` | one line: the intent of the row (shown in the report) |
| `action` / `inputs` | the interaction + any typed values (hostile values for `adversarial_`) |
| `expected_rpc` | `agent.v1.<Service>/<Method>`, or `local` (browser-only, no backend) |
| `expected_request` | assertion on the *decoded* request fields |
| `expected_outcome` | resulting widget state / dialog / snackbar / disabled-state; for Layer B also the read-RPC state delta |
| `layer` | `A` (all rows run hermetically) · `B` (mutating subset re-run live) |

## Case taxonomy (what every element must cover)

- **`positive_`** — the happy path; the action succeeds and the UI reflects it.
- **`negative_`** — the backend returns an error status; assert the right
  error/offline/retry state, **no crash, no silent success**.
- **`boundary_`** — empty/max/limit inputs (empty list, single item, max-length field,
  a disabled control that must *stay* disabled until its precondition is met).
- **`corner_`** — unusual-but-valid (unicode, CRLF, a builtin/locked entry, a
  content-deduped active graph).
- **`adversarial_`** — **mandatory for every text / number / raw-JSON input**, per
  [CLAUDE.md](../../../CLAUDE.md): traversal-looking ids, injection strings, RTL,
  gigantic payloads, invalid JSON. Assert the rejection / truncation / inert handling.

### The four async states (mandatory per async view)

Every page that loads over gRPC must have rows for **`loading`**, **`empty`**,
**`error`**, and **`loaded`** — the portal is full of these panels (offline banners,
empty states, retry buttons), and they are the states that "look broken."

### The resilience matrix (the `negative_`/`adversarial_` rows)

The fake server ([`portal-testkit`](03-layer-a-widget.md)) replays each gRPC status the
UI must survive; every mutating element gets a row per relevant status:

| injected status | expected UI |
|---|---|
| `UNAVAILABLE` | offline banner + retry, no crash |
| `UNIMPLEMENTED` | "not connected to <seam>" state (e.g. a bare gateway answering the Fleet tab) |
| `NOT_FOUND` | the specific empty/unavailable state (e.g. a draft with no body) |
| `PERMISSION_DENIED` | error snackbar, action not applied |
| `RESOURCE_EXHAUSTED` | error snackbar + backoff, no double-submit |
| **slow / hung** | loading state shown **and** the in-flight guard disables the button (`_switching`/`_sending`/`_saving`) — assert **no double-submit** |

## The exact-RPC-set rule

An assertion is not "some call happened." Each row pins:

1. **the method** that must fire (`expected_rpc`),
2. **the request fields** (`expected_request`), decoded from the wire by the fake, and
3. **that no *unintended* RPC fired** — the fake's recording is checked as a set, so a
   chatty regression (an extra list/refresh call) fails the row.

This is the precise, testable form of "the correct backend gRPC call was triggered."
Layer B adds the two live signals — the `agent_grpc_server_rpc_total{…,outcome="ok"}`
delta and the read-RPC state check (see [`04`](04-layer-b-e2e.md)).

---

## The element id / key scheme

Stable ids, assigned in the exploration inventory, become the widget `Key`s (increment
1). Format: `<page>.<element>[.<subpart>]`. They are the join key across the app code,
the spec tables, the report, and the perf table.

---

## Worked spec rows per page

Abbreviated tables — representative rows per element (the executable suite carries the
full `positive_/negative_/boundary_/corner_/adversarial_` set + the four async states).
`expected_rpc` is the authoritative element→method map.

### Launch — `LauncherPage` (no backend; always "up")

| element_id | example cases | expected_rpc | expected_outcome |
|---|---|---|---|
| `launch.card.grafana` `.hyperdx` `.prometheus` | `positive_open`, `negative_launch_blocked` | `local` (url_launcher) | opens the configured URL; on failure a SnackBar — never a crash |

### Prompts — `PromptsPage` (gateway · `PromptService`)

| element_id | example cases | expected_rpc | expected_request / outcome |
|---|---|---|---|
| (page load) | `loading` / `empty` / `error` / `loaded` | `PromptService/List` + `GetActivePersonality` | list grouped by kind; error → `_ErrorRetry` |
| `prompts.personality.dropdown` | `positive_switch`, `negative_unavailable` | `PromptService/SetActivePersonality` | request `{id, persist}`; disabled while `_switching` (assert no double-submit) |
| `prompts.personality.persist` | `positive_persist_flag` | `local` then next `SetActivePersonality` | `persist=true` on the next switch |
| `prompts.list.item` | `corner_builtin_locked` | `local` | loads content into editor; builtin shows lock |
| `prompts.save` | `positive_save`, `adversarial_huge_content` | `PromptService/Put` | request `{kind,id,content}`; reload after; huge content capped/handled |
| `prompts.delete` | `positive_delete`, `negative_missing` | `PromptService/Delete` | request `{kind,id}` |
| `prompts.preview` | `positive_preview` | `PromptService/PreviewAssembled` | request `{mode, goal}`; AlertDialog shows assembled text |

### Graph — `GraphPage` (gateway · `GraphService` + localStorage library)

| element_id | example cases | expected_rpc | expected_request / outcome |
|---|---|---|---|
| (page init) | `empty_seeds_examples`, `loaded` | `GraphService/DescribeNodeTypes` (+ best-effort `Get`) | empty library seeds the 4 shipped example graphs (local assets); server-active deduped by content |
| `graph.new` `.import` `.rename` `.duplicate` `.export` `.delete` | `positive_*`, `adversarial_import_bad_json` | `local` (localStorage / file) | library mutates; bad import rejected, no crash |
| `graph.node.add` | `boundary_blocked_when_no_types`, `positive_add` | `local` | blocked with a snack when node-types unfetched (backend down); else adds |
| `graph.node.params` (raw-JSON toggle) | `adversarial_giant_invalid_json` | `local` | invalid/huge JSON rejected in-editor |
| `graph.edge.add` `.remove` | `positive_*`, `corner_self_edge` | `local` | edge list mutates |
| `graph.validate` | `positive_valid`, `negative_invalid` | `GraphService/Validate` | request `{graph}`; issues dialog lists problems |
| `graph.setActive` | `positive_set` | `GraphService/Put` | request `{graph}`; refresh → active check |
| `graph.retry` | `error` state | `GraphService/DescribeNodeTypes` | offline banner clears on success |

### Agent — `AgentViewPage` (sessions · `AgentSession`+`SessionRegistry`; gateway · pool/metrics)

| element_id | example cases | expected_rpc | expected_request / outcome |
|---|---|---|---|
| (subscribe) | `loading` / `error(stream_down)` / `loaded` | `AgentSessionService/Subscribe` | stream drives transcript; error → "not receiving events" panel |
| `agent.send` | `positive_send`, `negative_registry_down` | `SessionRegistry/Open` **then** `AgentSession/Send` | Open req `{user:'portal'}`; Send req `{goal}`; disabled while `_sending` |
| `agent.reconnect` | `positive_resubscribe` | `AgentSessionService/Subscribe` | re-opens the stream |
| `agent.status.pool` | `loaded`, `error` | `LlmPoolService/Health` (3 s poll — fake clock) | pool cells populate; down → greyed |
| `agent.status.grpc` | `loaded` | `MetricsProxyService/Query` (5 s poll — fake clock) | p50/p99 cells from canned PromQL |

### Router — `RouterPage` (gateway · `ProviderRegistryService`; live-apply)

| element_id | example cases | expected_rpc | expected_request / outcome |
|---|---|---|---|
| (upstreams load) | four async states | `ProviderRegistry/List` | cards render; error → `_OfflineRetry` |
| `router.view.*` | `positive_switch_view` | `local` | segmented button swaps sub-view |
| `router.upstream.enable` | `positive_toggle` | `ProviderRegistry/Enable` | request `{id, enabled}` |
| `router.upstream.delete` | `positive_delete_confirm`, `negative_cancel` | `ProviderRegistry/Delete` | confirm dialog gates the call; cancel fires **no** RPC |
| `router.upstream.save` | `positive_save`, `adversarial_bad_url` | `ProviderRegistry/Put` | request = the draft upstream |
| `router.health.refresh` | `positive_refresh` | `ProviderRegistry/Health` | table repopulates |
| `router.route.run` | `positive_route` | `ProviderRegistry/Route` | request = the hint; decision card (introspection, no dispatch) |

### Fleet — `FleetPage` (fleet :8093 · `ReviewFleetService`)

| element_id | example cases | expected_rpc | expected_request / outcome |
|---|---|---|---|
| (list load) | four async states + `negative_unimplemented` (bare gateway) | `ReviewFleet/ListReviews` (+ best-effort `List`) | reviews + roster; UNIMPLEMENTED → "not connected to the fleet" |
| `fleet.filter.repo` `.status` `.refresh` | `positive_filter` | `ReviewFleet/ListReviews` | request carries `{repo,status,limit}` |
| `fleet.session.reviewNow` | `positive_review_now` | `ReviewFleet/ReviewNow` | PR-number dialog → request `{sessionId, prNumber}` |
| `fleet.session.enable` | `positive_toggle` | `ReviewFleet/SetEnabled` | request `{id, enabled}` |
| `fleet.review.item` / `fleet.detail` | `loaded`, `corner_unavailable_body` | `ReviewFleet/GetReview` | request `{reviewId}`; NotFound → unavailable state (Approve/editor hidden) |
| `fleet.detail.mode` | `boundary_locked_disables_edit` | `local` | Edit/Preview disabled when posted/approved |
| `fleet.detail.save` | `positive_save`, `boundary_disabled_unless_dirty` | `ReviewFleet/UpdateReview` | request `{reviewId, body}`; disabled unless dirty |
| `fleet.detail.approve` | `positive_approve`, `boundary_gated`, `adversarial_double_click` | `ReviewFleet/Approve` | confirm dialog → request `{reviewId}`; **exactly one** call on rapid double-click |
| (draft markdown image) | `adversarial_remote_image` | `local` | image is inert — **no remote fetch** |

### Settings — `SettingsPage` (gateway · `ConfigService`; write-TOML, restart-to-apply)

| element_id | example cases | expected_rpc | expected_request / outcome |
|---|---|---|---|
| (load) | four async states | `ConfigService/GetSchema` + `GetValues` + `Status` | sections list + form; banner from `Status` |
| `settings.section.item` | `positive_select`, `corner_dirty_dot` | `local` | selects section; orange dot when dirty |
| `settings.form` (`SchemaForm`) | `adversarial_bad_number`, `corner_secret_blank_keeps` | `local` (stages edits) | enum→dropdown, bool→switch, secret masked, blank secret = keep |
| `settings.validate` | `positive_valid`, `negative_issues` | `ConfigService/Validate` | request `{edits}`; issues dialog |
| `settings.save` | `positive_save`, `negative_issues` | `ConfigService/Put` | request `{edits}`; restart banner on success |
| `settings.revert` | `boundary_disabled_unless_dirty` | `local` | drops staged edits |
| `settings.retry` | `error` state | `GetSchema`+`GetValues` | reload |

---

## Unwired stub methods (documented, not tested via UI)

These exist in the generated stubs but no element invokes them
(`ProviderRegistry/GetPolicy`+`PutPolicy`; `ReviewFleet/Get`+`Put`+`Delete`+`Preflight`;
`AgentSession/Snapshot`; `SessionRegistry/Close`+`Heartbeat`; `LlmPool/Complete`;
`MetricsProxy/QueryRange`; `PromptService/Select`). The **contract drift guard**
([`03`](03-layer-a-widget.md)) records the *invoked* set so, if the UI later wires one,
its absence from the spec table is caught by the completeness critic.
