# 03 — Layer A: widget breadth (hermetic, parallel, seconds)

Layer A is the bulk of the suite: **every element × every case class**, running under
`flutter test` on the Dart VM, hermetic, sharded across isolates. It is the answer to
"exercise every option, in a few seconds, in parallel." It also carries the L0 unit
tests, the completeness critic, and the contract drift guard. Gated as the new
`portal-widget` check.

## `portal-testkit` — the in-process fake gRPC server

A new Dart library (the analogue of the Rust [`agent-testkit`](../../components/testing.md)
doubles crate) providing **recording, scriptable fakes** over the generated
`*ServiceBase` classes — one per seam the portal dials (`PromptService`,
`GraphService`, `ProviderRegistryService`, `ConfigService`, `AgentSessionService`,
`SessionRegistryService`, `ReviewFleetService`, `LlmPoolService`, `MetricsProxyService`).

Each fake:

- **records** every call as `(method, decodedRequest, ts)` into an ordered log the test
  reads back — this is what proves "the correct RPC fired with these args" and, checked
  as a set, "**no unintended RPC** fired";
- **scripts** responses per method: `ok(response)`, `error(StatusCode)`, `slow(delay)`
  (against the injected clock), or a boundary payload — driving the
  `positive_/negative_/boundary_/corner_/adversarial_` rows and the resilience matrix;
- binds a real `Server` on an **ephemeral loopback port** (`localhost:0`) so tests are
  parallel-safe; the page connects a real `ClientChannel` (the VM `channel_io` path).

A `FakeGateway` helper starts the subset of fakes a page needs, hands back a
`PortalConfig` pointing at their ports, and exposes the recording log + a per-method
response script. Builders produce canned domain objects (prompts, an example graph,
upstreams, a config schema+values, a review draft) — the Dart analogue of
`agent-testkit`'s `bench` fixtures; the shipped `config/cognition/*.textproto` graphs
are reused for graph data.

## The widget-key scheme (increment 1, a prerequisite)

The app has **no** test keys today. Increment 1 adds a `Key(...)` to every interactive
widget using the ids from [`02`](02-test-spec.md) (`prompts.save`,
`router.upstream.enable`, `graph.setActive`, `fleet.detail.approve`, `settings.save`,
…). Pure additive; no behavior change. These keys are the join across app code, spec
rows, report, and perf table, and are shared by Layer B (`find.byValueKey`).

## Per-page robots

Each page gets a robot (see [`01`](01-architecture.md#the-robot-page-object-pattern))
wrapping its `find.byKey` interactions behind intent methods. Tests call the robot; a
renamed key is fixed once. Robots are shared with Layer B.

## Test structure

```
portal/test/
  testkit/               # portal-testkit: fakes, FakeGateway, builders, robots
  unit/                  # L0 pure-logic tests
  pages/<page>_test.dart # L1 widget tests: iterate the page's spec rows
  meta/coverage_test.dart      # the completeness critic
  meta/contract_test.dart      # (or Rust) the contract drift guard
```

A page test iterates its spec table; per row it starts the `FakeGateway` with the row's
response script, pumps the page via its robot, performs the action, then asserts the
recorded RPC set + request + the widget outcome, and emits a report record
([`05`](05-report.md)). Because each row is independent and state is fresh, `flutter
test` shards them across isolates → the whole page suite runs in well under a second.

## L0 — pure-logic unit tests

No widget pump, microseconds each, highest bug-catch density:

- `graph_json.dart` — `graphToJson` / `graphFromJson` round-trip (+ the shipped example
  assets, reusing the #418 parity fixtures).
- `schema_form.dart` — field ↔ JSON mapping across enum/bool/string/number/nested/
  raw-JSON, secret masking, `$ref`/`allOf` resolution.
- Settings config-diff / dirty computation.
- `GraphLibrary` localStorage serialization round-trip.

## The completeness critic (guarantees "every option")

`meta/coverage_test.dart` enumerates every interactive `Key` present in `portal/lib`
and **fails** if any key has no `positive_` spec row. As the UI grows, a new element
cannot silently escape coverage — the same "no silent caps" discipline the Rust side
applies. (Implementation: a small key-registry the app exposes in test/debug, or a
source scan; the doc's contract is "no keyed element without a test.")

## The contract drift guard

A parity test (Dart here, or Rust mirroring the #418 graph parity test) asserts that the
**RPC set the portal invokes** and the **proto enums the UI renders** (task modes,
tiers, roles, review statuses) still match the generated descriptor set — so a proto
change can't silently strand the UI. It also records the *invoked* method set that the
completeness critic and [`02`](02-test-spec.md)'s "unwired stubs" note reference.

## The `portal-widget` nix check

A new `nix/checks/portal-widget.nix`, registered in
[`nix/checks/default.nix`](../../../nix/checks/default.nix) by direct import (like
`dart-analyze` — it needs `versions.flutter`, not craneLib):

- builds hermetically with `versions.flutter.buildFlutterApplication` + offline
  `autoPubspecLock` (the exact vendoring `dart-analyze.nix` uses), overridden to run
  `flutter test --machine` over `portal/test/` (the JSON reporter feeds the report);
- **`portal/test/` must be added to the check's source fileset** — today `dart-analyze`
  *excludes* it as the dead scaffold; the scaffold `widget_test.dart` is deleted in
  increment 2 and the real suite included;
- requires `flutter_test` + `integration_test` (SDK deps) in `portal/pubspec.yaml`,
  which means **regenerating the tracked `portal/pubspec.lock`** (see
  [`../../../nix/checks/dart-analyze.nix`](../../../nix/checks/dart-analyze.nix) and the
  `dart-analyze-gate` note — the lock is git-tracked to feed offline vendoring).
- `outputs = ["out"]; separateDebugInfo = false;` as `dart-analyze` needs, writing a
  success marker + the machine report to `$out`.

Runs on `nix flake check --max-jobs 8 --cores 4` and completes in seconds.
