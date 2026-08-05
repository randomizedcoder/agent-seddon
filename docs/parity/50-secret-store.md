# Parity spec 50 — secret store / keyring-backed credentials

Per-feature parity spec for a new **`SecretStore` seam**: a single, inspectable
place that hands out provider credentials — stored in the **OS keyring** (or an
encrypted local vault) instead of plaintext env vars and config files, populated
by an **OAuth login/device flow** where a provider supports it — under one
non-negotiable contract: **a secret is handed out, and it never leaks into a
result, an error, a log, or a span.**

> **Status: ⬜ spec written, not started.** Proposed: a new `SecretStore` async
> trait in `agent-core` (`get`/`set`/`delete`/`list` over a typed `SecretRef`),
> a new `agent-secrets` crate behind a `secrets-keyring` cargo feature wrapping
> the Rust [`keyring`](https://crates.io/crates/keyring) crate (macOS Keychain /
> Windows Credential Manager / Linux libsecret) plus an encrypted-file fallback
> vault, one `register_builtins` factory line, and a config key
> `[secrets] backend = "keyring" | "vault" | "env"`. Providers/forge/web-search
> stop reading `api_key`/`token` directly and instead resolve a `SecretRef`
> through the seam. **Differentiator:** the seam's *entire* contract is
> "hand out a secret, never leak it" — the redaction invariant already proven
> case-by-case for the forge token (spec 27) and the web-search key (spec 12) is
> lifted to a **seam-wide, typed** guarantee (a `Secret` newtype whose `Debug`
> prints `<redacted>`, errors that carry a `SecretRef` name but never a value)
> and **metered by access outcome** (`accessed`/`missing`/`refresh_failed`
> counts, labelled by *source* and *outcome* — never by key name or value). No
> peer exposes credential storage as a swappable, introspectable seam; codex has
> the richest storage/OAuth surface but hard-wires it into its login crate.
> **Deferred:** the OAuth authorization-code + device-code login flows (large,
> provider-specific — the seam takes a `TokenSource` unchanged; the keyring/vault
> storage half lands first), external secret managers (Bitwarden/1Password, as a
> later `SecretSource` backend), and the `secrets.proto` / `--serve-secrets`
> gRPC service, consistent with specs 11–37 (storage-only first, wire later).

## Feature & why it matters

Every provider call needs a credential, and today that credential lives in the
clear: an inline TOML string, an environment variable, or a file the agent reads
verbatim. Three problems follow, and each is a reason this is a **seam** rather
than a helper function:

- **At-rest exposure.** A plaintext `api_key` in `config/agent.toml`, an env var
  visible in `/proc/<pid>/environ` and every child process, or a `~/.key` file is
  a credential one `cat`, one `ps e`, one accidental `git add` from leaking. An
  OS keyring (Keychain / Credential Manager / libsecret) keeps the secret behind
  the platform's credential broker; an encrypted-file vault keeps it behind a
  passphrase. The agent should *ask a seam* for the credential and never hold the
  storage detail.
- **No login path.** Modern providers issue short-lived OAuth access tokens with
  a refresh token, not a static key — obtained by an authorization-code (PKCE) or
  device-code flow, then **refreshed** as they expire. agent-seddon has no way to
  perform that login or to refresh a token, so it cannot use an OAuth-only
  provider at all. A `SecretStore` that can back a `SecretRef` with a
  `TokenSource` (static key *or* a refreshable OAuth grant) closes that gap.
- **Leak surface is everywhere.** A credential passes through config load,
  factory wiring, an HTTP header, an error on failure, a metrics label, and a
  trace span. Today the "never logged" property is upheld **by convention** in
  each subsystem independently (the forge module re-derives it, the web-search
  module re-derives it, the tokenizer provider hand-writes a `Debug` impl). One
  seam that owns credential handout can make redaction a **type-level, tested,
  seam-wide** invariant instead of a rule everyone must remember.

The unit is a **credential**, resolved by name: `get(SecretRef) -> Secret`,
where `Secret` is a newtype that cannot be printed and whose only accessor hands
the raw bytes straight to an HTTP header builder. That handout — and its
non-leakage — is the whole contract.

## agent-seddon today

**No secret store, no keyring, no OAuth.** Credentials are resolved from three
plaintext sources at **provider-factory time** (not at config load):

- **Three plaintext sources, one resolver.**
  [`resolve_api_key`](../../crates/agent-runtime/src/builder.rs) (`builder.rs`,
  ~line 1391) tries, in order: an inline `ProviderCfg.api_key` string, the env
  var named by `api_key_env` (`std::env::var`), then the file named by
  `api_key_file` (read + `trim`); missing all three → a distinct early error
  (`"no API key: set provider.api_key, provider.api_key_env, or
  provider.api_key_file"`). A sibling
  [`resolve_ws_key`](../../crates/agent-runtime/src/registry.rs) (`registry.rs`,
  ~line 910) does the inline-then-env resolution for the forge token and Brave
  key (no file source; returns `""` when unset). The env-var *names* are
  themselves config-driven (`api_key_env = "ANTHROPIC_API_KEY"`), not hard-coded.
- **Keys reach only HTTP headers.**
  [`anthropic.rs`](../../crates/agent-providers/src/anthropic.rs) stores
  `api_key: String` and uses it once — `.header("x-api-key", &self.api_key)`;
  [`openai_compat.rs`](../../crates/agent-providers/src/openai_compat.rs) uses
  `.bearer_auth(&self.api_key)`. **Neither provider struct derives `Debug`**, so
  a `{:?}` can't print the key.
- **The "never leaks into results/errors/spans" invariant already exists —
  per-subsystem.** [`agent-forge/src/http.rs`](../../crates/agent-forge/src/http.rs)
  ("The token never leaves this module. Not into results, errors, spans, or
  logs.") builds status-code-only errors and uses the token only in the auth
  header (spec [`27-forge.md`](27-forge.md)).
  [`agent-web-search/src/http.rs`](../../crates/agent-web-search/src/http.rs)
  does the same for the Brave `X-Subscription-Token` (spec
  [`12-web-search.md`](12-web-search.md), test
  `secret_never_leaks_into_output_or_span`). The one place with a **hand-written
  redacting `Debug`** is
  [`agent-tokenizer/src/provider.rs`](../../crates/agent-tokenizer/src/provider.rs)
  (~line 39: `// Never derive Debug — it would print api_key.`,
  `finish_non_exhaustive()`). Spans/metrics are documented "never the API key"
  ([`agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs), label
  comments).
- **The un-typed gap.** `ProviderCfg` **does** `#[derive(Debug)]` and **does**
  hold the plaintext inline `api_key`
  ([`config.rs`](../../crates/agent-runtime/src/config.rs), ~line 1548), so a
  `{:?}` of a `Config` would print an inline key. There is **no** `Secret` /
  `SecretString` / `Sensitive` wrapper anywhere. The invariant is upheld by
  everyone remembering it, not by the type system — exactly the duplication a
  seam consolidates.
- **The seam scaffolding is all in place.** The trait+registry+decorator+double
  pattern is uniform: async traits in
  [`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`LlmProvider`,
  `Forge` ~2455, `Scanner` ~2821, `Policy` ~2845, `RepoBackend` ~3392, …), a
  `Factory<T>` table + `register_builtins`
  ([`registry.rs`](../../crates/agent-runtime/src/registry.rs), ~line 434), a
  `Metered*` decorator per seam
  ([`metered.rs`](../../crates/agent-runtime/src/metered.rs)), and scripted
  doubles in [`agent-testkit`](../../crates/agent-testkit/src/lib.rs) (e.g.
  `ScriptedWebSearch` ~line 807, with a `calls()` counter — the model for a
  `FakeKeyring`).

Honest gap: everything credential-*storage*-related is greenfield — no keyring,
no vault, no `SecretStore` trait, no OAuth login, no token refresh, no `Secret`
newtype. What exists is the plaintext resolver and a hand-maintained no-leak
convention; §6 is what to build, §7 is the plan of record.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/secrets/` (`SecretsBackend` trait: `get`/`set`/`delete`/`list`; `SecretsManager`; `LocalSecretsBackend` age/scrypt `*.age` vault), `codex-rs/keyring-store/` (`KeyringStore` trait + `DefaultKeyringStore` wrapping the `keyring` crate), `codex-rs/login/` (OAuth auth-code+PKCE `server.rs`, device-code `device_code_auth.rs`, refresh in `auth/manager.rs`, storage backends in `auth/storage.rs`), `codex-rs/aws-auth/` (`AwsAuthContext` SigV4/Bedrock) | `secrets/src/{lib,local,sanitizer}.rs` inline `mod tests`; `login/tests/suite/{auth_refresh,device_code_login,login_server_e2e,logout}.rs` + inline `auth/*_tests.rs`; `aws-auth/src/lib.rs` inline | cargo `#[test]` / `#[tokio::test]` (wiremock; no insta) |
| opencode | `packages/opencode/src/auth/index.ts` (`Oauth`/`Api`/`WellKnown` schema; `Service.get/all/set/remove` → **plaintext `auth.json`, mode 0600**; no OS keyring), OAuth in `provider/auth.ts` + `mcp/oauth-*.ts`; `Redacted` credentials in `packages/llm/src/route/auth.ts` | `packages/opencode/test/auth/auth.test.ts`, `packages/opencode/test/server/auth.test.ts`, `packages/core/test/oauth-page.test.ts`, `packages/opencode/test/mcp/oauth-*.test.ts` | bun:test + Effect |
| pi | `packages/ai/src/auth/credential-store.ts` (`CredentialStore` trait: `InMemoryCredentialStore` vs file-backed `FileAuthStorageBackend` in `packages/coding-agent/src/core/auth-storage.ts` → **plaintext `auth.json`, 0600 + lockfile**; no OS keyring), typed `ApiKeyCredential`/`OAuthCredential` in `auth/types.ts`, rich per-provider OAuth+device-code in `packages/ai/src/auth/oauth/` | `packages/coding-agent/test/auth-storage.test.ts`, `packages/ai/test/oauth-auth.test.ts`, `packages/ai/test/oauth-device-code.test.ts`, `packages/coding-agent/test/runtime-credentials.test.ts` | vitest |
| hermes | `hermes_cli/auth.py` (**plaintext `~/.hermes/auth.json`** + lock; PKCE/device-code for many providers), `agent/anthropic_adapter.py` (**macOS Keychain _read_** of Claude-Code creds via `security find-generic-password`), `agent/secret_sources/{bitwarden,onepassword}.py` (external managers), `agent/redact.py` (`RedactingFormatter`) | `tests/agent/test_anthropic_keychain.py`, `tests/hermes_cli/test_auth_commands.py`, `tests/hermes_cli/test_web_oauth_dispatch.py`, `tests/agent/test_credential_pool.py`, `tests/agent/test_redact.py` | pytest |

**codex** is the deep anchor — a purpose-built, layered credential stack, and the
only peer that writes to the OS keyring:

- **A `SecretsBackend` trait that *is* the seam** (`secrets/src/lib.rs`, ~line 90):
  `set`/`get` (→ `Option<String>`, so *missing is a typed `None`, never a
  panic*)/`delete` (→ `bool`)/`list`. `SecretsManager` (~97) wraps
  `Arc<dyn SecretsBackend>`; `SecretName` (validated charset), `SecretScope`
  (`Global` | `Environment(id)`) key credentials per-project. This is the exact
  shape agent-seddon's `SecretStore` copies.
- **Keyring behind a trait** (`keyring-store/src/lib.rs`): `KeyringStore`
  (`load`/`save`/`delete`) with `DefaultKeyringStore` wrapping the `keyring` crate
  (per-OS: `apple-native`/`windows-native`/`linux-native-async-persistent`), and
  `keyring::Error::NoEntry` mapped to `Ok(None)` — the *missing-secret-is-typed*
  contract. It logs `value_len`, **never the value**. A shared, non-`cfg(test)`
  `MockKeyringStore` (injectable `set_error`) is reused across crates — the model
  for our `FakeKeyring` double.
- **Encrypted vault fallback** (`secrets/src/local.rs`): secrets are an
  age/scrypt-encrypted `*.age` file; the scrypt passphrase is a random 32-byte key
  kept **in the keyring**. **Fails closed** — if the keyring is unavailable,
  `set`/`get` propagate the error (test
  `set_fails_when_keyring_is_unavailable`). Atomic temp-then-rename writes
  (`save_file_does_not_leave_temp_files`); schema-version gate
  (`load_file_rejects_newer_schema_versions`).
- **OAuth login + fail-closed refresh** (`login/`): auth-code + PKCE (S256) via a
  local callback server (`server.rs` `exchange_code_for_tokens`) and a device-code
  flow (`device_code_auth.rs`); `AuthManager` refresh (`auth/manager.rs`) splits
  failures into `RefreshTokenError::{Permanent, Transient}` — a 401 or a known
  reason (`refresh_token_expired`/`reused`/`invalidated`) is **Permanent and not
  retried**, and is cached per auth-snapshot so later attempts fail fast. Storage
  is swappable: `FileAuthStorage` (0600), `DirectKeyringAuthStorage`,
  `SecretsKeyringAuthStorage`, `AutoAuthStorage` (keyring-preferred, **file
  fallback on keyring error**), `EphemeralAuthStorage`.
- **Redaction, but only where hand-written** (the cautionary tale we make
  seam-wide): custom `Debug` on `AuthHeaders`, `PersonalAccessTokenAuth`,
  `SecretsKeyringAuthStorage`, `AuthManager`, `AwsAuthContext`; URL query-param
  scrubbing (`SENSITIVE_URL_QUERY_KEYS`) and a regex `redact_secrets` helper.
  **Caveat codex itself carries:** `TokenData`, `AuthDotJson`, `ApiKeyAuth`,
  `Chatgpt*` still **derive `Debug`** with plaintext token fields — a leak the
  moment one is formatted. agent-seddon's `Secret` newtype removes that footgun by
  construction. Tests to mirror:
  `raw_auth_client_does_not_log_sensitive_request_or_response_data`,
  `missing_auth_json_returns_none`,
  `refresh_token_returns_permanent_error_for_expired_refresh_token`,
  `refresh_token_does_not_retry_after_permanent_failure`,
  `refresh_token_returns_transient_error_on_server_failure`,
  `auto_auth_storage_load_falls_back_when_keyring_errors`.

**opencode** and **pi** both persist to a **plaintext `auth.json` (chmod 0600)** —
no OS keyring at all. Their value is the *shape*: opencode's `Auth.Service`
(`get/all/set/remove`) and `Redacted`-wrapped credentials at the LLM route
(`packages/llm/src/route/auth.ts`, unwrapped only at header-set time) mirror our
`Secret` newtype; pi ships the cleanest **pluggable** `CredentialStore` (an
`InMemoryCredentialStore` for tests vs a file backend for prod — exactly our
seam+double split) plus the richest per-provider OAuth/device-code suite
(`test/oauth-device-code.test.ts`: "honors a server-provided slow_down interval",
"cancels an in-flight wait"; `test/oauth-auth.test.ts`: "anthropic refresh
exchanges the refresh token and returns a typed credential"). Both are marked
"no OS keyring" — a real gap agent-seddon closes.

**hermes** is the second keyring data point but **read-only**: it *reads* the
macOS Keychain for Claude-Code credentials (`agent/anthropic_adapter.py`
`_read_claude_code_credentials_from_keychain` shelling `security
find-generic-password`; tests `test_returns_none_on_linux`,
`test_keychain_takes_priority_over_json_file`) and integrates external secret
managers (Bitwarden/1Password under `agent/secret_sources/`), but its own
credentials still land in plaintext `~/.hermes/auth.json`. Its standout is a
**dedicated redaction module** (`agent/redact.py` `RedactingFormatter` wired into
logging) and a credential pool that asserts env-seeded secrets are never
persisted (`test_load_pool_does_not_persist_env_seeded_secret_value`) — both
inform our metered/redacted contract.

Net: codex is the storage+OAuth anchor (and its unredacted-`Debug` structs are
the anti-pattern we type our way out of); opencode/pi contribute the seam shape
and OAuth-refresh cases; hermes contributes keyring-read + redaction-as-a-module.
None expose credential storage as a swappable, gRPC-served, **metered** seam.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete of the five (spec only —
do **not** implement here). Each maps to a test case in §7.

- **`SecretStore` seam** (spec only — do **not** implement here). New async trait
  in `agent-core`: `get(&SecretRef) -> Result<Secret, SecretError>`,
  `set(&SecretRef, Secret)`, `delete(&SecretRef) -> bool`,
  `list() -> Vec<SecretRef>`. `SecretRef` is a validated name + optional scope
  (port codex `SecretName`/`SecretScope`); `Secret` is the redacting newtype
  below. Impl in a new `agent-secrets` crate behind a `secrets-keyring` feature;
  one `register_builtins` factory line; config-selected.
- **`Secret` newtype, seam-wide redaction** (spec only — do **not** implement
  here). A `Secret(String)` whose `Debug`/`Display` print `Secret("<redacted>")`
  and whose only accessor (`expose()`/`into_header_value()`) hands the bytes
  straight to a header builder. Replaces per-subsystem hand-written `Debug`
  hygiene with one type; `SecretError` carries the `SecretRef` **name** but never
  a value. (Port codex `AuthHeaders`/`SecretsKeyringAuthStorage` redaction, but as
  a reusable type — closing the `ProviderCfg`/codex-`TokenData` derived-`Debug`
  gap.)
- **Keyring backend** (spec only — do **not** implement here). Wrap the Rust
  `keyring` crate (Keychain / Credential Manager / libsecret); `NoEntry` →
  `SecretError::Missing` (typed, not empty). (Port codex `keyring-store`.)
- **Encrypted-file vault fallback** (spec only — do **not** implement here). For
  headless/CI hosts with no keyring daemon: an encrypted file (age/scrypt or
  equivalent) with atomic temp-then-rename writes and a schema-version gate; the
  passphrase itself in the keyring when present. (Port codex `LocalSecretsBackend`.)
- **`env` compatibility backend** (spec only — do **not** implement here). A
  backend that resolves a `SecretRef` from today's inline/env/file sources, so the
  seam is drop-in and the current `resolve_api_key` behaviour is preserved as one
  selectable implementation (default in hermetic mode).
- **OAuth `TokenSource` + fail-closed refresh** (spec only — do **not** implement
  here). A `SecretRef` may be backed by a static key **or** a refreshable OAuth
  grant (auth-code+PKCE / device-code). On expiry the seam refreshes; a refresh
  failure is classified `Permanent` (401 / expired / reused / revoked → **do not
  retry, surface a typed error, do not serve a stale token**) vs `Transient`
  (network → retryable via [`agent-retry`](../../crates/agent-retry/src/lib.rs)).
  **Fails closed.** (Port codex `RefreshTokenError`; pi device-code slow_down.)
- **Provider/forge/web-search consume the seam** (spec only — do **not** implement
  here). `resolve_api_key`/`resolve_ws_key` are re-expressed as `SecretStore::get`
  over a `SecretRef`; the raw key never lands in a `ProviderCfg`/`ForgeCfg` field
  that derives `Debug`.
- **Metered + traced by outcome, never by value** (spec only — do **not**
  implement here). `secret_access_total{source, outcome}` where `source ∈
  {keyring, vault, env, oauth}` and `outcome ∈ {hit, missing, refresh_ok,
  refresh_failed, denied}`; a `secrets.get` span carrying `{ref_name, source,
  outcome}` — **never the secret, never the raw ref value beyond its name**.
  Label cardinality is bounded enums (repo hard rule,
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs)). (New — no peer meters
  credential access.)
- **Policy gate on `set`/`delete`** (spec only — do **not** implement here).
  Mutating stored credentials is a privileged action → route through the `Policy`
  seam ([`08-permissions-policy.md`](08-permissions-policy.md)); `get` is
  ungated but metered. (New; codex's login-restriction enforcement is the
  spiritual analogue.)
- **gRPC service** (spec only — do **not** implement here). `secrets.proto` with
  `Get`/`Set`/`Delete`/`List` (a served `Get` returns an **opaque handle /
  success flag**, never the secret over the wire unless the transport is a trusted
  local UDS) + reflection + `--serve-secrets`; the buf baseline bump.

## Table-driven test plan

New `#[rstest]` tables in the `agent-secrets` crate. Every case runs against a
**`FakeKeyring`** in-memory double added to
[`agent-testkit`](../../crates/agent-testkit/src/lib.rs) (a `HashMap<String,
Secret>` behind the `KeyringStore`-shaped trait, with an injectable
`set_error(KeyringErr)` and a `calls()` counter — modelled on codex's
`MockKeyringStore` and our `ScriptedWebSearch` ~line 807) so **no real OS
keychain is ever touched** — the suite is fully deterministic and hermetic. A
`ScriptedTokenSource` doubles the OAuth half (canned `refresh` result: `Ok`,
`Permanent`, or `Transient`). Prefixes: `positive_` succeeds, `negative_`
rejects, `corner_` odd-but-valid, `boundary_` at a limit. `(port: <peer>)` marks
a case mined from a peer test; `(new: agent-seddon)` is ours.

```rust
// FakeKeyring (agent-testkit): in-memory KeyringStore double, no OS keychain.
//   FakeKeyring { map: Mutex<HashMap<String, String>>, err: Option<KeyringErr>,
//                 calls: AtomicUsize }
//   - load/save/delete hit `map`; if `err` set, they return it (fail-closed test).
//   - calls() lets a test assert a probe made exactly N backend touches.

// ---- round-trip: set then get then delete, via the keyring backend ----------
#[rstest]
#[tokio::test]
async fn positive_set_get_delete_roundtrip() {                               // (port: codex manager_round_trips_local_backend)
    // set(ref, Secret("v")) -> get(ref) == "v" (via expose()) -> delete(ref)==true
    // -> get(ref) == Err(Missing). Assert secret_access_total{source=keyring,
    // outcome=hit} incremented, and the FakeKeyring stored the value (not empty).
}

// ---- missing secret is a distinct typed error, not empty/panic --------------
#[rstest]
#[case::negative_get_missing_is_typed(SecretRef::new("ABSENT"),  Err("Missing"))]  // (port: codex missing_auth_json_returns_none / keyring NoEntry->None)
#[case::positive_get_present(         SecretRef::new("PRESENT"), Ok("v"))]          // (port: codex)
#[tokio::test]
async fn get_missing_cases(#[case] r: SecretRef, #[case] expect: Result<&str, &str>) {
    // FakeKeyring seeded only with PRESENT=v. ABSENT -> SecretError::Missing{ref_name}
    // (a typed variant, never a panic, never an Ok("")). outcome=missing metered.
}

// ---- env compatibility backend preserves today's inline>env>file order ------
#[rstest]
#[case::positive_inline_wins(   Some("inline"), Some("env"),  Some("file"), "inline")]  // (new: agent-seddon; ports resolve_api_key precedence)
#[case::positive_env_when_no_inline(None,        Some("env"),  Some("file"), "env")]
#[case::positive_file_when_only_file(None,       None,         Some("file"), "file")]
#[case::negative_none_configured(None,           None,         None,         /*Err*/ "no secret")]
#[tokio::test]
async fn env_backend_precedence_cases(
    #[case] inline: Option<&str>, #[case] env: Option<&str>,
    #[case] file: Option<&str>,   #[case] expect: &str,
) { /* EnvSecretStore::get honours inline>env>file, else typed error (no key echoed) */ }

// ---- vault fallback: encrypted file when keyring absent, atomic, versioned ---
#[rstest]
#[tokio::test]
async fn positive_vault_roundtrip_when_no_keyring() {                        // (port: codex LocalSecretsBackend + save_file_does_not_leave_temp_files)
    // backend=vault over a tempdir; set/get/delete round-trips through the
    // encrypted file; assert NO leftover `.tmp-*` file after set (atomic rename).
}
#[rstest]
#[tokio::test]
async fn negative_vault_rejects_newer_schema_version() {                     // (port: codex load_file_rejects_newer_schema_versions)
    // hand-write a vault file with version = CURRENT+1 -> get() -> typed Err, no panic.
}

// ---- policy gates set/delete; get is ungated --------------------------------
#[rstest]
#[case::positive_set_allowed(Decision::Allow, /*stored=*/ true)]             // (new: agent-seddon; cf. spec 08)
#[case::negative_set_denied( Decision::Deny,  /*stored=*/ false)]            // (new)
#[tokio::test]
async fn policy_gate_cases(#[case] decision: Decision, #[case] stored: bool) {
    // set(ref, secret) through a fixed-Decision Policy double; Deny -> Err("denied")
    // and FakeKeyring untouched (nothing stored). get() stays ungated.
}

// ---- OAuth refresh: transient retries, permanent fails closed ---------------
#[rstest]
#[case::positive_refresh_ok(       Refresh::Ok("fresh"),  Ok("fresh"))]      // (port: pi "anthropic refresh exchanges the refresh token")
#[case::negative_refresh_transient(Refresh::Transient,    Err("transient"))] // (port: codex refresh_token_returns_transient_error_on_server_failure)
#[case::negative_refresh_permanent(Refresh::Permanent,    Err("permanent"))] // (port: codex refresh_token_returns_permanent_error_for_expired_refresh_token)
#[tokio::test]
async fn oauth_refresh_cases(#[case] outcome: Refresh, #[case] expect: Result<&str, &str>) {
    // ScriptedTokenSource returns `outcome` on refresh(); Transient is retryable,
    // Permanent is NOT retried and NEVER serves the stale token. outcome metered.
}

// --------------------------------------------------------------------------
// ADVERSARIAL — mandatory (this seam's entire contract is "never leak").
// --------------------------------------------------------------------------

// ---- a stored secret NEVER appears in Debug output --------------------------
#[rstest]
#[tokio::test]
async fn adversarial_secret_never_in_debug_output() {                        // (new: agent-seddon; ports codex AuthHeaders redaction, fixes its TokenData gap)
    let s = Secret::from("SUPERSECRET");
    assert_eq!(format!("{s:?}"), r#"Secret("<redacted>")"#);
    // Also: format the whole SecretStore, the SecretRef, and the config that
    // carries it; assert NONE of the strings contains "SUPERSECRET".
}

// ---- a stored secret NEVER appears in an error message ----------------------
#[rstest]
#[tokio::test]
async fn adversarial_secret_never_in_error_message() {                       // (new: agent-seddon; ports codex redact_secrets / hermes redact.py)
    // Force get/set/refresh error paths (keyring err, permanent refresh, bad vault).
    // Every SecretError renders with the ref *name* but assert its Display/Debug
    // contains NO substring of the secret value. (Mirrors forge/web-search
    // status-only errors, now enforced by the SecretError type.)
}

// ---- a stored secret NEVER appears in logs or spans -------------------------
#[rstest]
#[tokio::test]
async fn adversarial_secret_never_in_logs_or_spans() {                       // (port: codex raw_auth_client_does_not_log_sensitive_...; opencode Redacted)
    // Run set+get+refresh under agent-testkit captured_spans + a captured tracing
    // subscriber; assert no span attribute, event field, or metric label equals or
    // contains the secret value. Only {ref_name, source, outcome} are recorded.
}

// ---- a missing secret is a distinct typed error, never a panic/empty --------
#[rstest]
#[tokio::test]
async fn adversarial_missing_secret_is_typed_not_panic() {                   // (port: codex keyring NoEntry->Ok(None) / missing_auth_json_returns_none)
    // get() on an unset ref returns SecretError::Missing (typed); it MUST NOT
    // panic and MUST NOT return Ok(Secret("")). outcome=missing metered exactly once.
}

// ---- OAuth token-refresh failure fails closed -------------------------------
#[rstest]
#[tokio::test]
async fn adversarial_refresh_failure_fails_closed() {                        // (port: codex refresh_token_does_not_retry_after_permanent_failure)
    // Prime an expired grant; ScriptedTokenSource::Permanent on refresh.
    // Assert: get() -> Err (no token served), the stale token is NEVER handed out,
    // the permanent failure is cached (a 2nd get() makes NO new refresh call —
    // assert token_source.calls() unchanged), outcome=refresh_failed metered.
}

// ---- adversarial SecretRef name is rejected, not sanitized ------------------
#[rstest]
#[case::adversarial_ref_traversal("../../root/id")]                          // (new: agent-seddon; cf. safe_segment)
#[case::adversarial_ref_separator("a/b")]
#[case::adversarial_ref_empty("")]
#[case::boundary_ref_maxlen_ok(/* 128 chars */ "A")]                         // boundary: at the cap, accepted
fn adversarial_ref_validation_cases(#[case] name: &str) {
    // SecretRef::new rejects traversal/separators/empty (like codex SecretName's
    // validated charset); over-length is capped/rejected, never silently truncated.
}
```

gRPC roundtrip (extend
[`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)):
`Set` a secret over the wire (UDS), `List` it back (names only), `Delete` it,
`Get` a missing ref → typed `NOT_FOUND` — asserting the seam is identical
in-process vs. served **and that a served `Get` never puts the raw value in a
result field over a non-local transport** (the value-never-crosses-the-wire
guarantee is the streamed-frame analogue of every other seam's roundtrip).

Prefix legend (repo convention): `positive_` expected success, `negative_`
expected error, `corner_` odd-but-valid, `boundary_` at a limit; `adversarial_`
are **mandatory** for this untrusted, security-critical surface. `(port: <peer>)`
names the peer a case was mined from; `(new: agent-seddon)` marks the metered-
outcome, policy-gated-mutation, seam-wide-`Secret`-newtype, and env-precedence
guarantees with no peer analogue.

## Harness obligations

The implementing PR must satisfy all (follows #21–45):

- **Seam + registry:** `SecretStore` async trait + `Secret` newtype + `SecretRef`
  / `SecretError` types in [`agent-core`](../../crates/agent-core/src/lib.rs);
  impls in a new `agent-secrets` crate behind `secrets-keyring` / `secrets-vault`
  / `secrets-env` cargo features (keyring wraps the `keyring` crate; vault an
  age/scrypt-class file; env re-expresses today's resolver); a `FakeKeyring` +
  `ScriptedTokenSource` in [`agent-testkit`](../../crates/agent-testkit/src/lib.rs);
  one `Registry::secret_store` factory line per backend in `register_builtins`,
  config-selected via `[secrets] backend = …`. Providers/forge/web-search resolve
  a `SecretRef` through the seam instead of a plaintext field. Doc in
  `docs/components/secrets.md`.
- **Proto + gRPC:** `crates/agent-proto/proto/agent/v1/secrets.proto`
  (`Get`/`Set`/`Delete`/`List`; `Get` returns a success/handle, the raw value
  gated to trusted local transports) + `build.rs` entry + server/client in
  `agent-grpc` + `--serve-secrets` + reflection; extend `roundtrip.rs`; commit the
  `buf.image.binpb` bump via `nix run .#buf-image`; add the endpoint/port to
  `nix/constants.nix` → `nix run .#gen-constants`.
- **Metrics + OTel:** a `MeteredSecretStore` decorator in
  [`metered.rs`](../../crates/agent-runtime/src/metered.rs) recording
  `secret_access_total{source, outcome}` (bounded-enum labels — **never a key name
  or value**, per the `agent-metrics` cardinality rule) and a `secrets.get` /
  `secrets.set` span carrying `{ref_name, source, outcome}` — the #44
  span-attribute pattern, with the value categorically excluded.
- **Retry:** OAuth refresh backoff uses [`agent-retry`](../../crates/agent-retry/src/lib.rs)
  (never hand-rolled); only `Transient` refresh failures retry, `Permanent` fails
  closed and is cached (no re-refresh).
- **Bench (likely SKIP):** the seam is **keyring/IO/network-bound** (keychain
  broker, file crypto, OAuth HTTP) with no deterministic CPU hot path — document
  the iai bench skip, as [`27-forge.md`](27-forge.md) did. *Possible exception:*
  the pure `SecretRef` validation / `Secret`-redaction-`Debug` helper is CPU-only
  and could carry a tiny Ir-ceilinged bench if it shows up hot.
- **Leak:** a dhat `tests/leak.rs` (iteration-based, `dhat-heap` feature) over the
  **set → get → delete** path against `FakeKeyring` — assert a credential
  round-trip frees the decrypted plaintext buffer (the vault path zeroes/drops its
  key material, like codex's `wipe_bytes`) and stays under an allocation budget;
  wired in `nix/checks/leak.nix`.

## References

- **agent-seddon:**
  [`crates/agent-runtime/src/builder.rs`](../../crates/agent-runtime/src/builder.rs) (`resolve_api_key` ~1391, `expand_tilde` ~1414 — the plaintext resolver this seam supersedes),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`resolve_ws_key` ~910, `register_builtins` ~434, `Factory<T>` table),
  [`crates/agent-runtime/src/config.rs`](../../crates/agent-runtime/src/config.rs) (`ProviderCfg.api_key` ~1548 — the `#[derive(Debug)]`-with-plaintext-key gap),
  [`crates/agent-providers/src/anthropic.rs`](../../crates/agent-providers/src/anthropic.rs) / [`openai_compat.rs`](../../crates/agent-providers/src/openai_compat.rs) (header-only key use, no `Debug`),
  [`crates/agent-tokenizer/src/provider.rs`](../../crates/agent-tokenizer/src/provider.rs) (~39 — the one hand-written redacting `Debug` to generalise),
  [`crates/agent-forge/src/http.rs`](../../crates/agent-forge/src/http.rs) / [`crates/agent-web-search/src/http.rs`](../../crates/agent-web-search/src/http.rs) (the per-subsystem no-leak invariant to lift seam-wide),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (metered-seam pattern),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) (bounded-label counters),
  [`crates/agent-retry/src/lib.rs`](../../crates/agent-retry/src/lib.rs) (refresh backoff),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`ScriptedWebSearch` ~807 — model for `FakeKeyring`),
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs);
  dependencies: [`08-permissions-policy.md`](08-permissions-policy.md) (policy gate on mutation), [`12-web-search.md`](12-web-search.md) + [`27-forge.md`](27-forge.md) (the key-never-leaks invariant reused).
- **codex (anchor):** `codex-rs/secrets/src/{lib,local,sanitizer}.rs` (`SecretsBackend`/`SecretsManager`/`LocalSecretsBackend`, `redact_secrets`; tests `manager_round_trips_local_backend`, `set_fails_when_keyring_is_unavailable`, `load_file_rejects_newer_schema_versions`, `save_file_does_not_leave_temp_files`),
  `codex-rs/keyring-store/src/lib.rs` (`KeyringStore`/`DefaultKeyringStore` over the `keyring` crate, `NoEntry`→`Ok(None)`, `MockKeyringStore`),
  `codex-rs/login/src/{server,device_code_auth,pkce,token_data}.rs` + `login/src/auth/{manager,storage,revoke}.rs` (OAuth auth-code+PKCE + device-code + `RefreshTokenError::{Permanent,Transient}` + swappable storage),
  `codex-rs/login/tests/suite/{auth_refresh,device_code_login,login_server_e2e,logout}.rs` (`refresh_token_returns_permanent_error_for_expired_refresh_token`, `refresh_token_does_not_retry_after_permanent_failure`, `refresh_token_returns_transient_error_on_server_failure`, `auto_auth_storage_load_falls_back_when_keyring_errors`, `raw_auth_client_does_not_log_sensitive_request_or_response_data`),
  `codex-rs/aws-auth/src/lib.rs` (`AwsAuthContext` manual redacting `Debug`, SigV4/Bedrock; `credentials_provider_failures_are_retryable`, `load_rejects_empty_service_name`).
- **opencode:** `packages/opencode/src/auth/index.ts` (`Auth.Service` → plaintext `auth.json` 0600, no keyring), `packages/opencode/src/provider/auth.ts` + `mcp/oauth-*.ts`, `packages/llm/src/route/auth.ts` (`Redacted` credentials); tests `packages/opencode/test/auth/auth.test.ts`, `test/server/auth.test.ts`, `packages/core/test/oauth-page.test.ts`, `test/mcp/oauth-*.test.ts`.
- **pi:** `packages/ai/src/auth/credential-store.ts` (`CredentialStore`/`InMemoryCredentialStore`), `packages/coding-agent/src/core/auth-storage.ts` (`FileAuthStorageBackend` → plaintext `auth.json` 0600+lock), `packages/ai/src/auth/{types,oauth/*}.ts`; tests `packages/coding-agent/test/auth-storage.test.ts`, `packages/ai/test/oauth-auth.test.ts`, `test/oauth-device-code.test.ts`, `packages/coding-agent/test/runtime-credentials.test.ts`.
- **hermes:** `hermes_cli/auth.py` (plaintext `~/.hermes/auth.json` + PKCE/device-code), `agent/anthropic_adapter.py` (macOS Keychain _read_ of Claude-Code creds), `agent/secret_sources/{bitwarden,onepassword}.py` (external managers), `agent/redact.py` (`RedactingFormatter`); tests `tests/agent/test_anthropic_keychain.py`, `tests/hermes_cli/test_auth_commands.py`, `tests/hermes_cli/test_web_oauth_dispatch.py`, `tests/agent/test_credential_pool.py`, `tests/agent/test_redact.py`.
