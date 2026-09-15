//! Operational self-diagnosis (docs/design/doctor/).
//!
//! A reusable set of [`Probe`]s the agent runs against its own dependencies,
//! aggregated concurrently into a [`DoctorReport`]. This is how **agent-seddon
//! determines its own operational state** — an operator (or CI, or the fleet
//! `Preflight` RPC) asks the agent "are your dependencies healthy?" instead of
//! shelling out to `curl` / `clickhouse-client` / `pgrep`.
//!
//! Every probe is **fail-soft**: it reports an outcome — including a failure, as
//! [`ProbeStatus::Fail`] — rather than erroring, so one dead dependency never aborts
//! the report. Probes run concurrently and each network dial is bounded by
//! [`PROBE_TIMEOUT`] so an unreachable host can't hang the whole run. `detail`
//! carries a status *class* or an operator-supplied (trusted) config value like an
//! address or a path — **never a resolved secret** and never a full raw error body.

use crate::config::Config;
use agent_core::{DoctorReport, Probe, ProbeOutcome, ProbeStatus};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Per-probe network dial budget. Generous enough for a slow-but-alive dependency,
/// short enough that a dead host doesn't stall `agent doctor`.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Trim an untrusted/verbose error rendering to a short, single-line status detail.
/// Keeps the operator-useful reason without pasting a full server error body.
fn short_detail(s: &str) -> String {
    let one_line = s.split('\n').next().unwrap_or(s).trim();
    let mut out: String = one_line.chars().take(160).collect();
    if one_line.chars().count() > 160 {
        out.push('…');
    }
    out
}

/// Run every probe concurrently and collect the outcomes, preserving input order.
pub async fn run(probes: Vec<Arc<dyn Probe>>) -> DoctorReport {
    let outcomes = futures_util::future::join_all(probes.iter().map(|p| p.check())).await;
    DoctorReport { probes: outcomes }
}

/// The default probe set for a process-level `agent doctor`: config selections,
/// ClickHouse liveness (if telemetry is on), provider API-key resolvability, and a
/// non-billing provider reachability ping.
pub fn probes_for(config: &Config) -> Vec<Arc<dyn Probe>> {
    vec![
        Arc::new(ConfigProbe::new(config)),
        Arc::new(ClickHouseProbe::new(config)),
        Arc::new(ProviderKeyProbe::new(config)),
        Arc::new(ProviderReachProbe::new(config)),
    ]
}

/// Resolve the provider's API key to its value (inline > env > file), or empty if
/// none is configured. **Never logged or placed in a report** — only used to send
/// the reachability request. Mirrors the provider builder's precedence.
fn resolve_provider_key(cfg: &crate::config::ProviderCfg) -> String {
    if !cfg.api_key.is_empty() {
        return cfg.api_key.clone();
    }
    if !cfg.api_key_env.is_empty() {
        if let Ok(v) = std::env::var(&cfg.api_key_env) {
            if !v.is_empty() {
                return v;
            }
        }
    }
    if !cfg.api_key_file.is_empty() {
        if let Ok(s) = std::fs::read_to_string(&cfg.api_key_file) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    String::new()
}

/// Run the default probe set for `config`.
pub async fn diagnose(config: &Config) -> DoctorReport {
    run(probes_for(config)).await
}

// ---------------------------------------------------------------------------
// ConfigProbe — the config parsed into the typed schema; report the selections.
// ---------------------------------------------------------------------------

/// Reaching this probe means the config already parsed into the typed [`Config`]
/// (a malformed file fails earlier, before any probe runs), so this always reports
/// `Ok` and summarises which seam impls the config selected — the `--check-config`
/// value, folded into the doctor.
pub struct ConfigProbe {
    summary: String,
}

impl ConfigProbe {
    pub fn new(config: &Config) -> Self {
        Self {
            summary: format!(
                "provider={} context={} policy={} memory={} tokenizer={}",
                config.agent.provider,
                config.agent.context,
                config.agent.policy,
                config.memory.backend,
                config.tokenizer.backend,
            ),
        }
    }
}

#[async_trait]
impl Probe for ConfigProbe {
    fn name(&self) -> &str {
        "config"
    }
    async fn check(&self) -> ProbeOutcome {
        ProbeOutcome::new("config", ProbeStatus::Ok, self.summary.clone(), 0)
    }
}

// ---------------------------------------------------------------------------
// ClickHouseProbe — dial the telemetry/fleet ClickHouse and SELECT 1.
// ---------------------------------------------------------------------------

/// Liveness of the shared ClickHouse (telemetry sink + fleet draft store). When
/// `[telemetry] enabled` is off it is `Skipped` (not applicable), never a failure.
pub struct ClickHouseProbe {
    enabled: bool,
    addr: String,
    database: String,
    user: String,
    password: String,
}

impl ClickHouseProbe {
    pub fn new(config: &Config) -> Self {
        let t = &config.telemetry;
        Self {
            enabled: t.enabled,
            addr: t.clickhouse_url.clone(),
            database: t.database.clone(),
            user: t.user.clone(),
            password: t.password.clone(),
        }
    }
}

#[async_trait]
impl Probe for ClickHouseProbe {
    fn name(&self) -> &str {
        "clickhouse"
    }
    async fn check(&self) -> ProbeOutcome {
        if !self.enabled {
            return ProbeOutcome::new("clickhouse", ProbeStatus::Skipped, "telemetry disabled", 0);
        }
        let history = agent_telemetry::ClickHouseHistory::new(
            self.addr.clone(),
            self.database.clone(),
            self.user.clone(),
            self.password.clone(),
        );
        let start = Instant::now();
        let result = tokio::time::timeout(PROBE_TIMEOUT, history.ping()).await;
        let ms = start.elapsed().as_millis();
        match result {
            Ok(Ok(())) => ProbeOutcome::new(
                "clickhouse",
                ProbeStatus::Ok,
                format!("{} reachable", self.addr),
                ms,
            ),
            Ok(Err(e)) => ProbeOutcome::new(
                "clickhouse",
                ProbeStatus::Fail,
                short_detail(&e.to_string()),
                ms,
            ),
            Err(_) => ProbeOutcome::new(
                "clickhouse",
                ProbeStatus::Fail,
                format!(
                    "timed out after {}s dialing {}",
                    PROBE_TIMEOUT.as_secs(),
                    self.addr
                ),
                ms,
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// ProviderKeyProbe — is the configured provider's API key resolvable? (no network)
// ---------------------------------------------------------------------------

/// Whether the selected LLM provider has an API key it can resolve — inline, from
/// an env var, or from a file — **without ever reading the key's value into the
/// report**. Absent-entirely is `Warn` (a keyless local endpoint like Ollama is a
/// valid config), a configured-but-missing key *file* is `Fail` (a real misconfig).
/// This is a presence check, not a validity check — dialing the endpoint is
/// Increment 2's non-billing ping.
pub struct ProviderKeyProbe {
    api_key: String,
    api_key_env: String,
    api_key_file: String,
}

impl ProviderKeyProbe {
    pub fn new(config: &Config) -> Self {
        Self {
            api_key: config.provider.api_key.clone(),
            api_key_env: config.provider.api_key_env.clone(),
            api_key_file: config.provider.api_key_file.clone(),
        }
    }
}

#[async_trait]
impl Probe for ProviderKeyProbe {
    fn name(&self) -> &str {
        "provider-key"
    }
    async fn check(&self) -> ProbeOutcome {
        // Precedence mirrors the provider builder: inline > env > file.
        if !self.api_key.is_empty() {
            return ProbeOutcome::new("provider-key", ProbeStatus::Ok, "inline key set", 0);
        }
        if !self.api_key_env.is_empty() {
            return match std::env::var(&self.api_key_env) {
                Ok(v) if !v.is_empty() => ProbeOutcome::new(
                    "provider-key",
                    ProbeStatus::Ok,
                    format!("env {} set", self.api_key_env),
                    0,
                ),
                _ => ProbeOutcome::new(
                    "provider-key",
                    ProbeStatus::Warn,
                    format!("env {} unset or empty", self.api_key_env),
                    0,
                ),
            };
        }
        if !self.api_key_file.is_empty() {
            return match std::fs::read_to_string(&self.api_key_file) {
                Ok(s) if !s.trim().is_empty() => ProbeOutcome::new(
                    "provider-key",
                    ProbeStatus::Ok,
                    format!("file {} readable", self.api_key_file),
                    0,
                ),
                Ok(_) => ProbeOutcome::new(
                    "provider-key",
                    ProbeStatus::Warn,
                    format!("file {} is empty", self.api_key_file),
                    0,
                ),
                Err(_) => ProbeOutcome::new(
                    "provider-key",
                    ProbeStatus::Fail,
                    format!("file {} missing or unreadable", self.api_key_file),
                    0,
                ),
            };
        }
        ProbeOutcome::new(
            "provider-key",
            ProbeStatus::Warn,
            "no API key configured (ok for a keyless local endpoint)",
            0,
        )
    }
}

// ---------------------------------------------------------------------------
// ProviderReachProbe — non-billing reachability ping of the model endpoint.
// ---------------------------------------------------------------------------

/// Whether the configured LLM endpoint is reachable and its credential is accepted,
/// via a **non-billing** `GET {base_url}/models` (Increment 2). A provider kind with
/// no models endpoint (a `grpc` client, a pool/router wrapper) is `Skipped`. This
/// dials the network; the key is used to authenticate but never appears in the
/// report. Grades: reachable+accepted ⇒ Ok; reached-but-odd-status ⇒ Warn; auth
/// rejected or unreachable ⇒ Fail.
pub struct ProviderReachProbe {
    kind: String,
    base_url: String,
    api_key: String,
    version: String,
    insecure_tls: bool,
}

impl ProviderReachProbe {
    pub fn new(config: &Config) -> Self {
        Self {
            kind: config.agent.provider.clone(),
            base_url: config.provider.base_url.clone(),
            api_key: resolve_provider_key(&config.provider),
            version: config.provider.version.clone(),
            insecure_tls: config.provider.insecure_tls,
        }
    }

    /// A trusted (config-derived) endpoint label for the report — never the key.
    fn endpoint_label(&self) -> String {
        if self.base_url.is_empty() {
            format!("{} default endpoint", self.kind)
        } else {
            self.base_url.clone()
        }
    }
}

/// Map a [`Reach`](agent_providers::reach::Reach) grade to a probe status + detail.
/// Pure, so the grading policy is testable without a network dial: reachable+
/// accepted ⇒ Ok; reachable-but-odd-status ⇒ Warn; auth rejected or unreachable ⇒
/// Fail; a kind with no models endpoint ⇒ Skipped. `label` and `kind` are trusted
/// config values; the error text is truncated before display.
fn grade_reach(
    reach: agent_providers::reach::Reach,
    label: &str,
    kind: &str,
) -> (ProbeStatus, String) {
    use agent_providers::reach::Reach;
    match reach {
        Reach::Ok => (ProbeStatus::Ok, format!("{label} reachable")),
        Reach::AuthRejected(code) => (
            ProbeStatus::Fail,
            format!("{label} rejected the credential (http {code})"),
        ),
        Reach::BadStatus(code) => (
            ProbeStatus::Warn,
            format!("{label} reachable, unexpected http {code}"),
        ),
        Reach::Unreachable(e) => (
            ProbeStatus::Fail,
            format!("{label} unreachable: {}", short_detail(&e)),
        ),
        Reach::Unsupported => (
            ProbeStatus::Skipped,
            format!("no non-billing reachability endpoint for provider `{kind}`"),
        ),
    }
}

#[async_trait]
impl Probe for ProviderReachProbe {
    fn name(&self) -> &str {
        "provider-reach"
    }
    async fn check(&self) -> ProbeOutcome {
        use agent_providers::reach::{probe, ReachParams};
        let start = Instant::now();
        let reach = probe(ReachParams {
            kind: &self.kind,
            base_url: &self.base_url,
            api_key: &self.api_key,
            version: &self.version,
            insecure_tls: self.insecure_tls,
            timeout: PROBE_TIMEOUT,
        })
        .await;
        let ms = start.elapsed().as_millis();
        let (status, detail) = grade_reach(reach, &self.endpoint_label(), &self.kind);
        ProbeOutcome::new("provider-reach", status, detail, ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trivial probe double so the aggregator can be tested without any network.
    struct FakeProbe {
        name: &'static str,
        status: ProbeStatus,
    }

    #[async_trait]
    impl Probe for FakeProbe {
        fn name(&self) -> &str {
            self.name
        }
        async fn check(&self) -> ProbeOutcome {
            ProbeOutcome::new(self.name, self.status, "fake", 0)
        }
    }

    fn fake(name: &'static str, status: ProbeStatus) -> Arc<dyn Probe> {
        Arc::new(FakeProbe { name, status })
    }

    // --- aggregator: gate + ordering + counts ---

    #[rstest::rstest]
    // description, statuses in, expected ok(), expected fail-count
    #[case::positive_all_ok("all probes ok ⇒ gate passes", vec![ProbeStatus::Ok, ProbeStatus::Ok], true, 0)]
    #[case::positive_warn_and_skip_pass("warn + skip do not fail the gate", vec![ProbeStatus::Warn, ProbeStatus::Skipped], true, 0)]
    #[case::negative_one_fail("a single failure trips the gate", vec![ProbeStatus::Ok, ProbeStatus::Fail], false, 1)]
    #[case::negative_all_fail("every probe failed", vec![ProbeStatus::Fail, ProbeStatus::Fail], false, 2)]
    #[case::boundary_empty("no probes ⇒ vacuously ok", vec![], true, 0)]
    #[case::corner_fail_amid_skips("a fail hides among skips", vec![ProbeStatus::Skipped, ProbeStatus::Fail, ProbeStatus::Skipped], false, 1)]
    #[tokio::test]
    async fn aggregator_gate_cases(
        #[case] description: &str,
        #[case] statuses: Vec<ProbeStatus>,
        #[case] expected_ok: bool,
        #[case] expected_fail_count: usize,
    ) {
        let probes: Vec<Arc<dyn Probe>> = statuses
            .iter()
            .enumerate()
            .map(|(i, s)| fake(Box::leak(format!("p{i}").into_boxed_str()), *s))
            .collect();
        let report = run(probes).await;
        assert_eq!(report.ok(), expected_ok, "{description}");
        assert_eq!(
            report.count(ProbeStatus::Fail),
            expected_fail_count,
            "{description}"
        );
    }

    #[tokio::test]
    async fn positive_run_preserves_probe_order() {
        let probes = vec![
            fake("first", ProbeStatus::Ok),
            fake("second", ProbeStatus::Warn),
            fake("third", ProbeStatus::Skipped),
        ];
        let report = run(probes).await;
        let names: Vec<&str> = report.probes.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["first", "second", "third"]);
    }

    // --- ProbeOutcome::new clamps a hostile latency ---

    #[rstest::rstest]
    #[case::positive_normal("a normal latency passes through", 42u128, 42u32)]
    #[case::boundary_u32_max("exactly u32::MAX is preserved", u32::MAX as u128, u32::MAX)]
    #[case::adversarial_overflow("an absurd latency saturates, never panics", u128::MAX, u32::MAX)]
    fn probe_outcome_latency_clamp_cases(
        #[case] description: &str,
        #[case] latency_ms: u128,
        #[case] expected: u32,
    ) {
        let o = ProbeOutcome::new("x", ProbeStatus::Ok, "d", latency_ms);
        assert_eq!(o.latency_ms, expected, "{description}");
    }

    // --- short_detail: truncation of an untrusted/verbose error body ---

    #[rstest::rstest]
    #[case::positive_short(
        "a short detail is unchanged",
        "connection refused",
        "connection refused"
    )]
    #[case::corner_first_line_only(
        "only the first line survives",
        "line one\nline two",
        "line one"
    )]
    #[case::boundary_exactly_160("exactly 160 chars is not truncated", &["a"; 160].concat(), &["a"; 160].concat())]
    fn short_detail_cases(#[case] description: &str, #[case] input: &str, #[case] expected: &str) {
        assert_eq!(short_detail(input), expected, "{description}");
    }

    #[test]
    fn adversarial_short_detail_truncates_huge_body_with_ellipsis() {
        let huge = "x".repeat(10_000);
        let out = short_detail(&huge);
        assert_eq!(out.chars().count(), 161, "160 chars + ellipsis");
        assert!(out.ends_with('…'));
    }

    // --- ProviderKeyProbe: presence resolution (no network) ---

    #[tokio::test]
    async fn positive_provider_key_inline_is_ok() {
        let p = ProviderKeyProbe {
            api_key: "sk-xxxx".into(),
            api_key_env: String::new(),
            api_key_file: String::new(),
        };
        let o = p.check().await;
        assert_eq!(o.status, ProbeStatus::Ok);
        // The key value must never appear in the report.
        assert!(
            !o.detail.contains("sk-xxxx"),
            "detail must not leak the key"
        );
    }

    #[tokio::test]
    async fn negative_provider_key_none_configured_is_warn() {
        let p = ProviderKeyProbe {
            api_key: String::new(),
            api_key_env: String::new(),
            api_key_file: String::new(),
        };
        assert_eq!(p.check().await.status, ProbeStatus::Warn);
    }

    #[tokio::test]
    async fn negative_provider_key_missing_file_is_fail() {
        let p = ProviderKeyProbe {
            api_key: String::new(),
            api_key_env: String::new(),
            api_key_file: "/nonexistent/definitely/not/here.key".into(),
        };
        assert_eq!(p.check().await.status, ProbeStatus::Fail);
    }

    // --- ProviderReachProbe: grading policy (pure, no network) ---

    #[rstest::rstest]
    // description, reach grade, expected status
    #[case::positive_ok("2xx ⇒ Ok", agent_providers::reach::Reach::Ok, ProbeStatus::Ok)]
    #[case::negative_auth(
        "auth rejected ⇒ Fail",
        agent_providers::reach::Reach::AuthRejected(401),
        ProbeStatus::Fail
    )]
    #[case::corner_bad_status(
        "odd status ⇒ Warn",
        agent_providers::reach::Reach::BadStatus(404),
        ProbeStatus::Warn
    )]
    #[case::negative_unreachable("unreachable ⇒ Fail", agent_providers::reach::Reach::Unreachable("connection refused".into()), ProbeStatus::Fail)]
    #[case::boundary_unsupported(
        "unsupported kind ⇒ Skipped",
        agent_providers::reach::Reach::Unsupported,
        ProbeStatus::Skipped
    )]
    fn grade_reach_cases(
        #[case] description: &str,
        #[case] reach: agent_providers::reach::Reach,
        #[case] expected: ProbeStatus,
    ) {
        let (status, _detail) = grade_reach(reach, "http://h/v1", "openai-compat");
        assert_eq!(status, expected, "{description}");
    }

    #[test]
    fn adversarial_grade_reach_truncates_unreachable_body() {
        let huge = "x".repeat(10_000);
        let (status, detail) = grade_reach(
            agent_providers::reach::Reach::Unreachable(huge),
            "http://h/v1",
            "openai-compat",
        );
        assert_eq!(status, ProbeStatus::Fail);
        // Prefix + truncated body + ellipsis — bounded, not the whole 10k.
        assert!(
            detail.chars().count() < 300,
            "detail must be bounded: {}",
            detail.len()
        );
    }

    #[tokio::test]
    async fn boundary_provider_reach_unsupported_kind_is_skipped_no_network() {
        // A `grpc` provider has no models endpoint — Skipped without any dial.
        let p = ProviderReachProbe {
            kind: "grpc".into(),
            base_url: "http://127.0.0.1:1/v1".into(),
            api_key: "sk-secret".into(),
            version: String::new(),
            insecure_tls: false,
        };
        let o = p.check().await;
        assert_eq!(o.status, ProbeStatus::Skipped);
        assert!(!o.detail.contains("sk-secret"), "must not leak the key");
    }

    // --- resolve_provider_key: precedence + never-empty-on-present ---

    #[test]
    fn positive_resolve_key_prefers_inline() {
        let cfg = crate::config::ProviderCfg {
            api_key: "inline".into(),
            api_key_env: "SHOULD_NOT_READ".into(),
            api_key_file: "/nope".into(),
            ..provider_cfg_stub()
        };
        assert_eq!(resolve_provider_key(&cfg), "inline");
    }

    #[test]
    fn negative_resolve_key_none_configured_is_empty() {
        let cfg = provider_cfg_stub();
        assert_eq!(resolve_provider_key(&cfg), "");
    }

    /// A minimal `ProviderCfg` for the key-resolution tests (fields we don't touch
    /// get any valid default).
    fn provider_cfg_stub() -> crate::config::ProviderCfg {
        crate::config::ProviderCfg {
            base_url: String::new(),
            model: "m".into(),
            version: "2023-06-01".into(),
            api_key: String::new(),
            api_key_env: String::new(),
            api_key_file: String::new(),
            insecure_tls: false,
            max_retries: 0,
            supports_vision: false,
        }
    }

    // --- ClickHouseProbe: disabled telemetry is Skipped, never a failure ---

    #[tokio::test]
    async fn boundary_clickhouse_disabled_is_skipped() {
        let p = ClickHouseProbe {
            enabled: false,
            addr: "localhost:9000".into(),
            database: "agent".into(),
            user: "default".into(),
            password: String::new(),
        };
        let o = p.check().await;
        assert_eq!(o.status, ProbeStatus::Skipped);
    }
}
