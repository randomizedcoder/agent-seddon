# Parity spec 45 — doctor / self-diagnostics

Per-feature parity spec for a **`Diagnostic` seam** + a **`doctor` CLI
subcommand**: a battery of read-only environment/config checks — toolchain,
`protoc`, provider keys reachable, listen ports free, sandbox backends
available, gRPC endpoints — each reporting **pass / warn / fail** with a
remediation hint, so a broken setup is *diagnosed up front* rather than stumbled
into on the first real run.

> **Status: ⬜ spec written, not started.** Proposed: a new **`Diagnostic`
> seam** (an async trait in `agent-core`; a registry of small, individually
> inspectable checks wired exactly like tools/seams in
> [`register_builtins`](../../crates/agent-runtime/src/registry.rs)) + a
> **`doctor` CLI subcommand** in [`crates/agent-cli/src/main.rs`](../../crates/agent-cli/src/main.rs)
> that runs the registered checks, aggregates them into a pass/warn/fail
> report, and exits non-zero on any `fail` — plus a `doctor.checks` config key
> selecting which checks run and a `doctor_checks_total{check,status}` metric.
> **Differentiator:** each diagnostic is a *pure function of injected
> environment probes* with its own metric, and the doctor is **itself a seam** —
> checks are registered like tools, so they are individually testable with a
> fake environment (no real network/toolchain), remotable over gRPC
> (`--serve-doctor`), and reflection-introspectable, exactly like every other
> agent-seddon seam. **Deferred:** a `--fix`/auto-remediation mode (hermes has
> one; we keep doctor strictly read-only for the first cut), a `codex update`-style
> self-update check, per-check reachability probes that dial *live* provider
> endpoints under `nix flake check` (the offline probe is injected/faked here —
> a networked variant is a follow-up gated like the VCR provider matrix), and a
> `doctor.proto` streaming report (the unary aggregate report is the right first
> shape). This is the design of record; **none of it exists yet** (bar the narrow
> config-file validator noted below).

## Feature & why it matters

agent-seddon has a rich, feature-gated surface — two providers, a search index
(tantivy needs a recent rustc), gRPC codegen (`tonic-build` needs `protoc`),
sandbox backends (nix / bubblewrap / docker), a dozen `--serve-<seam>` gateways
that each bind a port or UDS — and today the *first* time any of that is wrong,
you learn about it as a mid-run failure: a provider 401 three turns in, a
`bind: address already in use` when a gateway starts, a sandbox that silently
falls back to the host because `bwrap` isn't on `PATH`. That is exactly the
class of problem a `doctor` command exists to surface **before** the work
starts.

The unit is a **check**: a named, read-only probe that inspects one facet of the
environment/config and returns a `{status, summary, remediation}` row. A
`doctor` run is the aggregate — run every registered check, print a
pass/warn/fail table, exit non-zero if anything failed:

- **Toolchain** — is a compatible `rustc`/`cargo` reachable, is `protoc`
  present (gRPC codegen needs it), is the pinned toolchain from `nix/versions.nix`
  actually the one on `PATH` (the #1 "why won't it build" footgun this repo's
  `CLAUDE.md` already warns about)?
- **Provider credentials** — for the configured provider(s), is the API key env
  var **present** (not *what it is* — see the adversarial case), is the base URL
  well-formed? A missing/blank key is a `fail`; a key present but pointing at a
  non-default endpoint is a `warn`.
- **Ports / sockets free** — for each `--serve-<seam>` endpoint the config would
  bind (ports/sockets come from `nix/constants.nix`), is the port bindable / the
  UDS path writable, or is something already listening?
- **Sandbox backends** — for the configured isolation backend (spec 14), is the
  executor (`bwrap`/`docker`/nix) actually available, or would the agent silently
  run unconfined on the host?
- **gRPC endpoints** — for a `= "grpc"`-configured seam client, is the target
  dialable (a bounded connect probe), and does reflection list the expected
  service?

Reporting `pass`/`warn`/`fail` per check — with a one-line remediation — turns a
class of silent, deferred failures into a single actionable up-front summary.

## agent-seddon today

**No environment/config doctor exists.** The CLI's flag parsing is hand-rolled
in [`crates/agent-cli/src/main.rs`](../../crates/agent-cli/src/main.rs) and the
full flag set is `--config, --continue, --resume, --scheduler, --check-config,
--serve-mcp, --serve-all, --serve-sessions, --review, --gate, --detect-mode,
--listen, --help`. There is **no `doctor`/`diagnose` subcommand** that inspects
the environment. The two closest things are both narrower:

- **`--check-config` validates the config *file* only.**
  ([`main.rs`](../../crates/agent-cli/src/main.rs) ~line 147: `Mode::CheckConfig`
  captures the selected impls, builds/validates the config via the
  [`agent-validate`](../../crates/agent-validate/) crate, and returns *before the
  run* — "a validated config is the whole job for `--check-config`".) It answers
  "does this TOML parse and select real impls?" — **not** "is the toolchain/keys/
  ports/sandbox environment those impls need actually present and healthy?".
- **The gRPC serving-health service** ([`crates/agent-grpc/src/server/health.rs`](../../crates/agent-grpc/src/server/health.rs))
  reports liveness of an *already-running* served seam. It is a runtime
  liveness probe for a live process, **not** a pre-flight environment doctor you
  run before starting anything.

Neither is an environment doctor. Honest gap: there is no self-diagnostic that
checks whether `rustc`/`protoc`/the pinned toolchain are reachable, whether the
configured provider's key is present, whether a `--serve-<seam>` port is free,
or whether the configured sandbox backend actually exists on the host.

**Reusable scaffolding, though, is already here:**

- **The plugin registry is the exact shape a check registry wants.**
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs) (`registry.rs`
  ~line 434) already maps a config string → factory for every seam, guarded by a
  cargo feature. A `Diagnostic` registry is the same pattern: register each check
  by name, config selects which run — "checks registered like tools".
- **Metered-seam + per-check metric pattern.**
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) already owns the
  counter-vec machinery; a `doctor_checks_total{check,status}` counter is one more
  family, and hostile numbers are already clamped before any `inc_by`.
- **The seam→gRPC lift is a well-worn path.** Every other seam gets a
  `--serve-<seam>` + reflection (`agent-grpc`); a `--serve-doctor` that runs the
  same registered checks remotely is wiring an established pattern.

Honest gap: the `Diagnostic` trait, the check registry, the individual checks,
the `doctor` subcommand + report aggregation, the metric, and any gRPC surface
**do not exist yet**.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/cli/src/doctor.rs` + `codex-rs/cli/src/doctor/{system,runtime,git,background,updates,title,thread_inventory,progress,output}.rs` (`codex doctor` subcommand; `DoctorCheck` with `CheckStatus{Ok,Warning,Fail}`, per-facet `*_check()` fns, `--json` redacted support report, `redact_detail`) | `codex-rs/cli/src/doctor/output.rs` `#[cfg(test)] mod tests` (~30 cases) + snapshot `codex-rs/cli/src/doctor/snapshots/codex__doctor__output__tests__doctor_human_report_environment_rows.snap`; adjacent `codex-rs/cli/tests/update.rs` | cargo `#[test]` + `insta` snapshots |
| hermes | `hermes_cli/doctor.py` (`run_doctor`, `check_ok`/`check_warn`/`check_fail`/`check_info` = ✓/⚠/✗/→, provider-key/toolset/gateway/cert/version/install checks) + `hermes_cli/subcommands/doctor.py` (`hermes doctor`, `--fix`, `--ack`) | `tests/hermes_cli/test_doctor.py` (~40 cases), `test_doctor_command_install.py`, `test_doctor_dedicated_provider_skip.py`, `tests/computer_use/test_doctor.py` | pytest (`monkeypatch` env + `tmp_path`) |
| opencode | — (no doctor/health-check *command*; CLI handlers are `serve/default/migrate/api/providers/debug/service` only — `doctor` appears solely as i18n strings, a DB-migration name, and `lsp/diagnostic.ts` LSP diagnostics, none an environment/config doctor) | — | bun:test |
| pi | — (no environment/config doctor; `packages/coding-agent/src/core/diagnostics.ts` is LSP-style **resource-collision** diagnostics and `packages/ai/src/utils/diagnostics.ts` is error-info extraction — neither a setup health command) | — | vitest |

**codex** is the anchor — a first-class, read-mostly `codex doctor` that is
*structured exactly the way this spec proposes*, and it pins the behaviours we
need:

- **A check is a serializable row, not a `print`.** `doctor.rs` (~line 202)
  models each check as a `DoctorCheck { category, status, summary, issues,
  details, remediation }` with `enum CheckStatus { Ok, Warning, Fail }` (~line
  175). The doctor header comment states the discipline outright: "checks inspect
  the current installation, configuration, authentication, terminal, state paths,
  and bounded reachability probes **without attempting repair**"; "a failing
  check should describe the problem and remediation, but it should **not mutate
  user state**." That read-only stance is the whole reason it is safe to run
  before filing a bug.
- **Checks are per-facet functions.** `system_check()` (`system.rs`),
  `runtime_check()` / `search_check()` (`runtime.rs`), `git_check()` (`git.rs`),
  `updates_check()` (`updates.rs`), `terminal_title_check()` (`title.rs`),
  `thread_inventory_check()` (`thread_inventory.rs`), `background_server_check()`
  (`background.rs`) — each returns one `DoctorCheck`; `run_doctor` (~line 307)
  aggregates them and **exits non-zero when `overall_status == Fail`** (~line 327).
  This is exactly "a registry of small inspectable checks" — codex just wires them
  by hand rather than through a registry.
- **The environment is *detected into an inputs struct*, then checked.**
  `SystemCheckInputs::detect()` (`system.rs` ~line 21) gathers OS/locale/editor/
  pager env into a plain struct that the pure check logic consumes — the exact
  seam that makes a check testable with a **fake** environment instead of the real
  one.
- **Secret redaction is a first-class, tested concern (the adversarial case's
  anchor).** `redact_detail` (`output.rs` ~line 832) never emits a secret's
  value: a detail whose key matches `openai_api_key`/`codex_api_key`/
  `codex_access_token`/`authorization`/`bearer_token`/`token`/`secret` renders as
  `"<name>: <redacted>"`, while a **presence** value (`is_safe_presence_value`:
  `present`/`absent`/`missing`/`not set`/`true`/`false`) is preserved verbatim.
  The `--json` support report is redacted the same way (`redacted_json_report`,
  ~line 619) so it is safe to paste into a bug. Tests pin this precisely:
  `redact_detail_preserves_secret_presence_booleans`,
  `redact_detail_sanitizes_secret_url_path_segments`,
  `redact_detail_preserves_env_var_names`, `redact_detail_sanitizes_urls`,
  `render_human_report_includes_redacted_details`.
- **Rendering is snapshot-tested.** `render_human_report` +
  `render_human_report_snapshot_covers_environment_rows` against
  `…__doctor_human_report_environment_rows.snap`, plus ascii/color/summary
  variants (`render_human_report_supports_ascii_output`,
  `render_human_report_explains_terminal_warning_issue`,
  `render_human_report_promotes_notes_without_changing_statuses`).

**hermes** ships a second, independent data point: `hermes doctor` (`--fix`,
`--ack`) in `hermes_cli/doctor.py`, with `check_ok`/`check_warn`/`check_fail`
(✓/⚠/✗) helpers and `_section` grouping. Its checks are the same *kinds* this
spec proposes — **provider credentials** (`_has_provider_env_config`, the
`_PROVIDER_ENV_HINTS` env-var battery, an offline/probe-safe key check),
**tool/toolset availability**, **gateway service** health
(`_check_gateway_service_linger`, `_check_s6_supervision`), **certificates**
(`check_certificates`), **version consistency** (`_check_version_consistency`),
and **install integrity** (symlink target). Its pytest suite is the direct model
for *our* fake-env table: `test_detects_openai_api_key` /
`test_returns_false_when_no_provider_settings` (key present vs absent),
`test_missing_symlink_shows_fail` / `test_wrong_target_symlink_shows_warn`
(fail vs warn on the same facet), and the auth cases
`test_token_env_present_shows_ok` /
`test_no_token_and_not_gh_authenticated_shows_warn` — every case drives the check
through `monkeypatch`'d env + a `tmp_path`, i.e. **injected, deterministic
inputs**, never the real machine. Notably hermes offers `--fix` (auto-remediate)
— which this spec **defers**; our first cut is read-only like codex.

**opencode** and **pi** have no environment/config doctor (both "—" above): this
is a feature where codex is the deep anchor, hermes a second confirming data
point, and agent-seddon can leapfrog both on distribution (the checks are a
**seam** — remotable over gRPC + reflection, `--serve-doctor`) and observability
(a per-check `doctor_checks_total{check,status}` metric), while porting codex's
read-only discipline and secret-redaction rigor wholesale.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **`Diagnostic` seam.** New async trait in `agent-core`:
  `name() -> &str`, `run(&self, env: &DiagEnv) -> CheckResult` where
  `CheckResult { status: Status, summary, remediation, details }` and
  `enum Status { Pass, Warn, Fail }` (port codex `CheckStatus`). Each check is a
  small impl in the owning crate behind its feature; **`DiagEnv` is an injected
  probe bundle** (env-var lookup, `which`-style PATH probe, a port-bind probe, a
  bounded gRPC connect probe, a filesystem probe) so a check is a *pure function
  of its inputs* — the real `DiagEnv` reads the machine; a `FakeEnv` double makes
  every test deterministic with **no real network/toolchain**. (Port codex
  `SystemCheckInputs::detect()` → pure-check split.)
- **Check registry (checks registered like tools).** A registry mapping check
  name → factory, one line per check in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs), guarded by
  the owning crate's feature; `doctor.checks` in `config/agent.toml` selects which
  run (default: all compiled-in). The doctor *is a seam* — this is the
  differentiator over codex's hand-wired list. (New: agent-seddon.)
- **The check battery.** `toolchain` (rustc/cargo/`protoc` reachable + pinned-vs-
  ambient), `provider_keys` (configured provider's key **present**, base URL
  well-formed), `ports` (each `--serve-<seam>` endpoint from `nix/constants.nix`
  bindable / UDS writable), `sandbox` (configured isolation backend executor
  present — else it'd run unconfined; cite [`14-sandbox.md`](14-sandbox.md)),
  `grpc_endpoints` (each `= "grpc"` client target dialable + reflection lists the
  service). (Port codex `system`/`runtime`/`search`/`background` checks + hermes
  provider/gateway checks.)
- **Aggregate report + exit code.** `doctor` runs the selected checks, prints a
  pass/warn/fail table with per-check remediation, and **exits non-zero iff any
  check is `Fail`** (a `Warn` alone is exit-0). A `--json` variant emits the same
  rows machine-readably. (Port codex `run_doctor` overall-status→exit-code.)
- **Secret non-leakage (mandatory, adversarial).** A check reports credential
  **presence only** — `present`/`absent` — and **never** the key's value into the
  summary, details, remediation, or `--json`. Port codex `redact_detail` +
  `is_safe_presence_value`: presence booleans pass through, anything matching a
  secret key renders `<redacted>`. This is the leak-critical path (the model /
  a pasted bug report reads doctor output). (Port codex
  `redact_detail_preserves_secret_presence_booleans`.)
- **Per-check metric + span (differentiator).** A
  `doctor_checks_total{check,status}` counter in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) (inc once per check per
  run) and a `doctor.check` OTel span (attrs `check`, `status`, `duration_ms`)
  reusing [`agent-telemetry`](../../crates/agent-telemetry/) — no peer has a
  metered doctor. (New: agent-seddon.)
- **gRPC surface.** `doctor.proto` with a unary `RunChecks(RunChecksRequest)
  returns (DoctorReport)` RPC + reflection + `--serve-doctor`; a remote doctor is
  dialable like any other seam (the checks run *on the server's* environment).
  (New — no peer offers a remotable doctor.)
- **Read-only invariant (ported discipline).** No check mutates user state
  (no writes, no `--fix`, no starting long-lived services) — safe to run against a
  broken install. `--fix` auto-remediation (hermes has it) is explicitly deferred.
  (Port codex "read-mostly, does not mutate".)

## Table-driven test plan

New `#[rstest]` tables in the doctor crate (check results + report aggregation),
plus a gRPC roundtrip case. **The load-bearing design point: every check is a
pure function of an injected `DiagEnv`**, so each case constructs a **`FakeEnv`**
(env vars, fake PATH set, fake port-bind outcomes, fake gRPC-connect outcomes)
and asserts the resulting `CheckResult` — **no real network, no real toolchain,
no wall-clock, no host inspection**. Doubles from
[`agent-testkit`](../../crates/agent-testkit/src/lib.rs): `tempdir()` for
filesystem-probe roots and a **new `FakeEnv`** builder. Prefixes: `positive_`
succeeds, `negative_` rejects/fails, `corner_` odd-but-valid, `boundary_` edge.
`(port: <peer>)` marks a case mined from a peer test; `(new: agent-seddon)` are
ours.

```rust
// ---- a passing check: toolchain present -----------------------------------
#[rstest]
#[case::positive_toolchain_all_present(
    FakeEnv::builder().on_path(&["rustc", "cargo", "protoc"]).build(),
    Status::Pass,
)]                                                                            // (port: codex runtime_check / system_check)
#[case::positive_provider_key_present(
    FakeEnv::builder().env("OPENAI_API_KEY", "sk-xxxx").provider("openai").build(),
    Status::Pass,
)]                                                                            // (port: hermes test_detects_openai_api_key)
#[tokio::test]
async fn positive_check_passes(#[case] env: FakeEnv, #[case] want: Status) {
    // run the check against the injected env; assert CheckResult.status == want
    // and doctor_checks_total{check,status="pass"} incremented exactly once.
}

// ---- a failing check: required credential missing --------------------------
#[rstest]
#[case::negative_provider_key_missing(
    FakeEnv::builder().provider("openai").unset("OPENAI_API_KEY").build(),
    Status::Fail,
)]                                                                            // (port: hermes test_returns_false_when_no_provider_settings)
#[case::negative_protoc_missing(
    FakeEnv::builder().on_path(&["rustc", "cargo"]).build(), // no protoc
    Status::Fail,
)]                                                                            // (port: codex runtime/search check)
#[case::negative_serve_port_in_use(
    FakeEnv::builder().port_bind("50051", BindOutcome::AddrInUse).build(),
    Status::Fail,
)]                                                                            // (new: agent-seddon) --serve-<seam> port probe
#[tokio::test]
async fn negative_check_fails(#[case] env: FakeEnv, #[case] want: Status) {
    // failing check surfaces status=Fail + a non-empty remediation string,
    // and mutates NO state (read-only invariant: FakeEnv records zero writes).
}

// ---- a warn: degraded-but-usable (non-default endpoint, missing sandbox) ---
#[rstest]
#[case::corner_provider_key_present_nondefault_base_url(
    FakeEnv::builder().env("OPENAI_API_KEY", "sk-x").env("OPENAI_BASE_URL", "http://lan:1234").build(),
    Status::Warn,
)]                                                                            // (port: hermes custom-endpoint / codex Warning)
#[case::corner_sandbox_backend_absent_falls_back_to_host(
    FakeEnv::builder().config_sandbox("bubblewrap").on_path(&[]).build(), // no bwrap
    Status::Warn,
)]                                                                            // (new: agent-seddon; cf. spec 14)
#[tokio::test]
async fn warn_check_degraded(#[case] env: FakeEnv, #[case] want: Status) {
    // degraded config yields Warn (not Fail): usable, but flagged with guidance.
}

// ---- ADVERSARIAL: a check must NEVER leak a secret value -------------------
#[rstest]
#[case::adversarial_present_key_reports_presence_not_value(
    FakeEnv::builder().env("OPENAI_API_KEY", "sk-SUPER-SECRET-abcdef").provider("openai").build(),
)]                                                                            // (port: codex redact_detail_preserves_secret_presence_booleans)
#[case::adversarial_secret_never_in_json_report(
    FakeEnv::builder().env("ANTHROPIC_API_KEY", "sk-ant-DEADBEEF").provider("anthropic").build(),
)]                                                                            // (port: codex redacted_json_report)
#[tokio::test]
async fn adversarial_no_secret_in_report(#[case] env: FakeEnv) {
    // run provider_keys check; serialize BOTH the human row and the --json row.
    // MANDATORY: the raw key value ("SUPER-SECRET"/"DEADBEEF") appears in NEITHER
    // summary, details, remediation, nor json; only "present"/"absent" is emitted.
    // (redaction is by key-name match + presence-value allowlist, ported from codex.)
}

// ---- aggregate report: overall status + exit code --------------------------
#[rstest]
#[case::boundary_all_pass_exit_zero(&[Status::Pass, Status::Pass], 0)]        // (port: codex run_doctor)
#[case::boundary_a_warn_alone_exit_zero(&[Status::Pass, Status::Warn], 0)]    // (port: codex Warning != Fail)
#[case::negative_any_fail_exit_nonzero(&[Status::Pass, Status::Fail], 1)]     // (port: codex overall_status == Fail)
fn report_aggregation_exit_code(#[case] statuses: &[Status], #[case] want_exit: i32) {
    // build a DoctorReport from injected per-check statuses; overall = worst,
    // exit code is non-zero iff any Fail (a lone Warn stays exit-0).
}

// ---- registry: only configured checks run ----------------------------------
#[rstest]
#[case::corner_config_selects_subset(&["toolchain"], /*ran=*/ &["toolchain"])] // (new: agent-seddon) doctor.checks
#[case::boundary_empty_selection_runs_nothing(&[], &[])]                        // (new: agent-seddon)
fn registry_selection(#[case] configured: &[&str], #[case] want_ran: &[&str]) {
    // doctor.checks in config selects which registered checks execute;
    // an unknown check name is rejected at config-validate time (spec: agent-validate).
}
```

gRPC roundtrip (extend [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)):
call `RunChecks` over the wire (TCP + UDS) with a server built over a **`FakeEnv`**
(so the served checks are deterministic — the checks run on the *server's*
injected environment), and assert the returned `DoctorReport` is byte-identical
to the in-process report for the same env — the pattern every other seam's
roundtrip test uses (the point is the seam is identical in-process vs. served),
**and** re-assert the adversarial invariant on the wire: no secret value crosses
the RPC boundary.

Prefix legend (repo convention): `positive_` expected success, `negative_`
expected fail, `corner_` odd-but-valid (→ Warn), `boundary_` at a limit.
`(port: <peer>)` names the peer a case was mined from; `(new: agent-seddon)`
marks the port-probe, sandbox-fallback, registry-selection, and metered-check
assertions that have no peer analogue.

## Harness obligations

The implementing PR must satisfy all (follows the #21–44 seam-add pattern):

- **Seam + registry:** `Diagnostic` trait + `CheckResult`/`Status` in
  `agent-core`; each check impl in its owning crate behind that crate's feature;
  one factory line per check in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs); a `FakeEnv`
  double + builder in [`agent-testkit`](../../crates/agent-testkit/src/lib.rs);
  the `doctor.checks` key added to `config/agent.toml` and validated by
  [`agent-validate`](../../crates/agent-validate/) (unknown check name → config
  error); doc in `docs/components/doctor.md`.
- **CLI:** a `doctor` subcommand in
  [`crates/agent-cli/src/main.rs`](../../crates/agent-cli/src/main.rs)'s
  hand-rolled parser (a new `Mode::Doctor`, sibling to `Mode::CheckConfig`) that
  runs the report and **exits non-zero on any `Fail`**; a `--json` flag for the
  machine-readable redacted report; `--help` text updated.
- **Proto + gRPC:** `crates/agent-proto/proto/agent/v1/doctor.proto`
  (unary `RunChecks(RunChecksRequest) returns (DoctorReport)`) + `build.rs` entry
  + server/client in `agent-grpc` + `--serve-doctor` + reflection; commit the
  `buf.image.binpb` bump (`nix run .#buf-image`); add the endpoint to
  `nix/constants.nix` → `nix run .#gen-constants`.
- **Metrics + OTel:** a `doctor_checks_total{check,status}` counter in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) (hostile-number-safe
  `inc_by`) + a `doctor.check` span (attrs `check`, `status`, `duration_ms`)
  reusing [`agent-telemetry`](../../crates/agent-telemetry/) — the metered-doctor
  differentiator.
- **Security (leak-critical):** the redaction path (secret-key-name match +
  presence-value allowlist, ported from codex `redact_detail` /
  `is_safe_presence_value`) is covered by the **mandatory `adversarial_` cases**
  asserting no key value reaches the human report, the `--json` report, or the
  wire. Read-only invariant asserted (checks perform zero writes).
- **Bench (likely SKIP):** the doctor path is **I/O / probe-bound** (PATH lookups,
  a port bind, a bounded gRPC connect) with no deterministic CPU hot path —
  document the iai bench skip, as `bash` did in
  [`04-shell-bash.md`](04-shell-bash.md). (If the redaction routine is extracted
  as a pure function, that alone is a candidate deterministic bench.)
- **Leak:** a dhat `tests/leak.rs` (`dhat-heap` feature) over a full
  `run_all_checks(FakeEnv)` iteration, asserting the report + per-check buffers
  are freed and the run stays under an allocation budget.

## References

- **agent-seddon:**
  [`crates/agent-cli/src/main.rs`](../../crates/agent-cli/src/main.rs) (hand-rolled flag parser + `Mode::CheckConfig` — the config-file-only validator this seam is *not*; where `Mode::Doctor` is added),
  [`crates/agent-validate/`](../../crates/agent-validate/) (the narrow config validator; validates the new `doctor.checks` key),
  [`crates/agent-grpc/src/server/health.rs`](../../crates/agent-grpc/src/server/health.rs) (runtime liveness of a *running* served seam — not a pre-flight doctor),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins` — the config-string→factory pattern the check registry mirrors),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (metered-seam pattern),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) (counter-vec to extend with `doctor_checks_total`),
  [`crates/agent-telemetry/`](../../crates/agent-telemetry/) (per-check span),
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs) (roundtrip pattern),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`tempdir`, doubles; new `FakeEnv`),
  dependencies: [`14-sandbox.md`](14-sandbox.md) (sandbox-backend availability check), [`08-permissions-policy.md`](08-permissions-policy.md), [`23-tokenizer-cost.md`](23-tokenizer-cost.md) (provider/cost config the credential check inspects), [`04-shell-bash.md`](04-shell-bash.md) (bench-skip precedent); `nix/versions.nix` + `nix/constants.nix` (pinned toolchain + serve endpoints the `toolchain`/`ports` checks read).
- **codex (anchor):** `codex-rs/cli/src/doctor.rs` (`run_doctor`, `DoctorCheck`, `enum CheckStatus { Ok, Warning, Fail }`, overall-status→exit-code, `--json` redacted support report, read-only discipline in the module doc),
  `codex-rs/cli/src/doctor/system.rs` (`SystemCheckInputs::detect()` → pure `system_check()` — the detect/check split that makes a check fake-env testable),
  `codex-rs/cli/src/doctor/{runtime,git,background,updates,title,thread_inventory}.rs` (`runtime_check`/`search_check`/`git_check`/`background_server_check`/`updates_check`/`terminal_title_check`/`thread_inventory_check`),
  `codex-rs/cli/src/doctor/output.rs` (`render_human_report`, `redact_detail` ~line 832, `is_safe_presence_value`, `redacted_json_report`);
  tests: `codex-rs/cli/src/doctor/output.rs` `#[cfg(test)] mod tests` (`redact_detail_preserves_secret_presence_booleans`, `redact_detail_sanitizes_secret_url_path_segments`, `redact_detail_preserves_env_var_names`, `redact_detail_sanitizes_urls`, `render_human_report_includes_redacted_details`, `render_human_report_snapshot_covers_environment_rows`, `render_human_report_supports_ascii_output`, `render_human_report_explains_terminal_warning_issue`, `render_human_report_promotes_notes_without_changing_statuses`), snapshot `codex-rs/cli/src/doctor/snapshots/codex__doctor__output__tests__doctor_human_report_environment_rows.snap`; adjacent self-update: `codex-rs/cli/src/doctor/updates.rs` + `codex-rs/cli/tests/update.rs`.
- **hermes (second data point):** `hermes_cli/doctor.py` (`run_doctor`, `check_ok`/`check_warn`/`check_fail`/`check_info`, `_has_provider_env_config` + `_PROVIDER_ENV_HINTS`, `_check_gateway_service_linger`, `_check_s6_supervision`, `check_certificates`, `_check_version_consistency`, install-symlink check), `hermes_cli/subcommands/doctor.py` (`hermes doctor`, `--fix`, `--ack`);
  tests: `tests/hermes_cli/test_doctor.py` (`test_detects_openai_api_key`, `test_returns_false_when_no_provider_settings`, `test_missing_api_key_summary_ignores_disabled_toolsets`, `test_token_env_present_shows_ok`, `test_no_token_and_not_gh_authenticated_shows_warn`, `test_check_gateway_service_linger_warns_when_disabled`), `tests/hermes_cli/test_doctor_command_install.py` (`test_missing_symlink_shows_fail`, `test_wrong_target_symlink_shows_warn`, `test_correct_symlink_shows_ok`, `test_fix_creates_missing_symlink`), `tests/hermes_cli/test_doctor_dedicated_provider_skip.py`, `tests/computer_use/test_doctor.py`.
- **opencode:** — (no doctor/health-check command; CLI handlers `serve/default/migrate/api/providers/debug/service`; `doctor` appears only as i18n strings, a DB-migration name, and `packages/opencode/src/lsp/diagnostic.ts` LSP diagnostics — none an environment/config doctor).
- **pi:** — (no environment/config doctor; `packages/coding-agent/src/core/diagnostics.ts` = LSP-style resource-collision diagnostics, `packages/ai/src/utils/diagnostics.ts` = error-info extraction — neither a setup health command).
