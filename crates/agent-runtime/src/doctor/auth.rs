//! Auth probes for `agent doctor` (security-hardening S11b): is the token signer
//! loadable, can each login issuer's keys be fetched, is the session store
//! reachable, and are the gRPC TLS certificates inside their validity window.
//!
//! Like every probe these are fail-soft and never put key material or a raw error
//! body in `detail`: a key id (a public thumbprint), a path, a count or a short
//! reason.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use agent_core::{Probe, ProbeOutcome, ProbeStatus};
use agent_grpc::server::{IssuerKeys, IssuerParams};
use async_trait::async_trait;

use super::{short_detail, PROBE_TIMEOUT};
use crate::config::Config;

/// Every auth probe `config` calls for: nothing under `mode = "none"` except the
/// TLS certificates, which matter with or without login.
pub(super) fn probes_for(config: &Config) -> Vec<Arc<dyn Probe>> {
    let mut out: Vec<Arc<dyn Probe>> = Vec::new();
    if config.auth.mode == "oidc" {
        out.push(Arc::new(SignerProbe::new(config)));
        out.extend(
            crate::auth_params::login_issuers(&config.auth)
                .into_iter()
                .map(|p| Arc::new(IssuerProbe::new(p)) as Arc<dyn Probe>),
        );
        out.push(Arc::new(SessionStoreProbe::new(config)));
    }
    out.push(Arc::new(TlsCertProbe::new(config)));
    out
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// --- the token signer ---------------------------------------------------------------

/// `[auth.token] signing_key` (and `previous_key`) load as the token service would
/// load them. Skipped without `[auth.token]`.
pub(crate) struct SignerProbe {
    /// `(label, path)` per configured key.
    keys: Vec<(&'static str, PathBuf)>,
}

impl SignerProbe {
    pub(crate) fn new(config: &Config) -> Self {
        let keys = config
            .auth
            .token
            .iter()
            .flat_map(|t| {
                [
                    ("signing_key", &t.signing_key),
                    ("previous_key", &t.previous_key),
                ]
                .into_iter()
                .filter(|(_, p)| !p.trim().is_empty())
                .map(|(label, p)| (label, PathBuf::from(p.trim())))
            })
            .collect();
        Self { keys }
    }
}

/// Group/other permission bits on a key file, if any are set.
fn loose_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).ok()?.permissions().mode() & 0o777;
    (mode & 0o077 != 0).then_some(mode)
}

#[async_trait]
impl Probe for SignerProbe {
    fn name(&self) -> &str {
        "auth.signer"
    }

    async fn check(&self) -> ProbeOutcome {
        let start = Instant::now();
        if self.keys.is_empty() {
            return ProbeOutcome::new(
                self.name(),
                ProbeStatus::Skipped,
                "no [auth.token] (this process issues no tokens)",
                start.elapsed().as_millis(),
            );
        }
        let mut status = ProbeStatus::Ok;
        let mut parts = Vec::new();
        for (label, path) in &self.keys {
            match agent_grpc::server::SigningKey::load(path) {
                Err(e) => {
                    status = ProbeStatus::Fail;
                    parts.push(format!("{label}: {}", short_detail(&e)));
                }
                Ok(key) => match loose_mode(path) {
                    Some(mode) => {
                        if status == ProbeStatus::Ok {
                            status = ProbeStatus::Warn;
                        }
                        parts.push(format!(
                            "{label}: kid {} but mode {mode:o} lets group/other read it",
                            key.kid()
                        ));
                    }
                    None => parts.push(format!("{label}: kid {}", key.kid())),
                },
            }
        }
        ProbeOutcome::new(
            self.name(),
            status,
            parts.join("; "),
            start.elapsed().as_millis(),
        )
    }
}

// --- login issuers ------------------------------------------------------------------

/// One login issuer's published keys can be fetched (through discovery when the
/// issuer has no `jwks_url`), exactly as the verifier fetches them.
pub(crate) struct IssuerProbe {
    name: String,
    params: IssuerParams,
}

impl IssuerProbe {
    pub(crate) fn new(params: IssuerParams) -> Self {
        Self {
            name: format!("auth.issuer.{}", params.name.trim()),
            params,
        }
    }
}

/// Grade a key-set fetch: some keys ⇒ Ok; an empty set can verify nothing ⇒ Fail.
pub(crate) fn grade_issuer(result: Result<IssuerKeys, String>) -> (ProbeStatus, String) {
    match result {
        Ok(k) if k.keys == 0 => (
            ProbeStatus::Fail,
            "published key set is empty: no login can verify".into(),
        ),
        Ok(k) => (
            ProbeStatus::Ok,
            format!(
                "{} key{} via {}",
                k.keys,
                if k.keys == 1 { "" } else { "s" },
                if k.discovered {
                    "discovery"
                } else {
                    "jwks_url"
                }
            ),
        ),
        Err(reason) => (ProbeStatus::Fail, short_detail(&reason)),
    }
}

#[async_trait]
impl Probe for IssuerProbe {
    fn name(&self) -> &str {
        &self.name
    }

    async fn check(&self) -> ProbeOutcome {
        let start = Instant::now();
        // Discovery then the key set: two requests, each bounded; the outer bound
        // covers a slow DNS lookup the client timeout does not.
        let fetched = tokio::time::timeout(
            PROBE_TIMEOUT * 2,
            agent_grpc::server::probe_issuer_keys(&self.params, PROBE_TIMEOUT),
        )
        .await
        .unwrap_or_else(|_| Err(format!("timed out after {}s", PROBE_TIMEOUT.as_secs() * 2)));
        let (status, detail) = grade_issuer(fetched);
        ProbeOutcome::new(self.name(), status, detail, start.elapsed().as_millis())
    }
}

// --- the session store --------------------------------------------------------------

/// `[auth.token] session_store` answers. In-memory sessions work but vanish on
/// restart and are not shared between processes, so they warn.
pub(crate) struct SessionStoreProbe {
    store: Option<String>,
    backend: Result<Option<agent_grpc::server::SessionBackend>, String>,
}

impl SessionStoreProbe {
    pub(crate) fn new(config: &Config) -> Self {
        let metrics = agent_metrics::Metrics::new();
        Self {
            store: config.auth.token.as_ref().map(|t| t.session_store.clone()),
            backend: crate::builder::resolve_auth_session_backend(config, &metrics)
                .map_err(|e| e.to_string()),
        }
    }
}

#[async_trait]
impl Probe for SessionStoreProbe {
    fn name(&self) -> &str {
        "auth.sessions"
    }

    async fn check(&self) -> ProbeOutcome {
        let start = Instant::now();
        let done = |status, detail: String| {
            ProbeOutcome::new("auth.sessions", status, detail, start.elapsed().as_millis())
        };
        let Some(store) = self.store.as_deref() else {
            return done(ProbeStatus::Skipped, "no [auth.token] (no sessions)".into());
        };
        let backend = match &self.backend {
            Err(e) => return done(ProbeStatus::Fail, short_detail(e)),
            Ok(None) | Ok(Some(_)) if matches!(store, "" | "memory") => {
                return done(
                    ProbeStatus::Warn,
                    "in memory: sessions end on restart and are not shared between \
                     processes (set session_store = \"file\" or \"postgres\")"
                        .into(),
                )
            }
            Ok(None) => return done(ProbeStatus::Fail, "no session store was built".into()),
            Ok(Some(b)) => b.0.clone(),
        };
        match tokio::time::timeout(
            PROBE_TIMEOUT,
            backend.tenants(agent_grpc::server::SESSION_COLLECTION),
        )
        .await
        {
            Ok(Ok(tenants)) => done(
                ProbeStatus::Ok,
                format!(
                    "{store}: reachable, {} tenant(s) with sessions",
                    tenants.len()
                ),
            ),
            Ok(Err(e)) => done(
                ProbeStatus::Fail,
                format!("{store}: {}", short_detail(&e.to_string())),
            ),
            Err(_) => done(
                ProbeStatus::Fail,
                format!("{store}: timed out after {}s", PROBE_TIMEOUT.as_secs()),
            ),
        }
    }
}

// --- TLS certificates ---------------------------------------------------------------

/// The gRPC listener's and client's certificates (`[grpc.tls] cert`,
/// `[grpc.tls.client] cert`) are inside their validity window. Warns once less
/// than a third of the lifetime is left (when a renewal at two thirds should
/// already have happened), so a 24-hour step-ca leaf and a one-year one are judged
/// alike.
pub(crate) struct TlsCertProbe {
    certs: Vec<(&'static str, PathBuf)>,
}

impl TlsCertProbe {
    pub(crate) fn new(config: &Config) -> Self {
        let t = &config.grpc.tls;
        let certs = [("listener", &t.cert), ("client", &t.client.cert)]
            .into_iter()
            .filter(|(_, p)| !p.trim().is_empty())
            .map(|(label, p)| (label, PathBuf::from(p.trim())))
            .collect();
        Self { certs }
    }
}

/// Grade one certificate's validity window at `now`.
pub(crate) fn grade_validity(
    validity: Result<(u64, u64), String>,
    now: u64,
) -> (ProbeStatus, String) {
    const DAY: u64 = 86_400;
    let (from, until) = match validity {
        Ok(v) => v,
        Err(e) => return (ProbeStatus::Fail, short_detail(&e)),
    };
    let left = |secs: u64| {
        if secs >= DAY {
            format!("{} days", secs / DAY)
        } else {
            format!("{} hours", secs / 3600)
        }
    };
    if now < from {
        return (
            ProbeStatus::Fail,
            format!("not valid for another {}", left(from - now)),
        );
    }
    if now >= until {
        return (
            ProbeStatus::Fail,
            format!("expired {} ago", left(now - until)),
        );
    }
    let remaining = until - now;
    if remaining.saturating_mul(3) < until - from {
        (
            ProbeStatus::Warn,
            format!("expires in {}: renew it", left(remaining)),
        )
    } else {
        (ProbeStatus::Ok, format!("expires in {}", left(remaining)))
    }
}

#[async_trait]
impl Probe for TlsCertProbe {
    fn name(&self) -> &str {
        "tls.certs"
    }

    async fn check(&self) -> ProbeOutcome {
        let start = Instant::now();
        if self.certs.is_empty() {
            return ProbeOutcome::new(
                self.name(),
                ProbeStatus::Skipped,
                "no [grpc.tls] certificate",
                start.elapsed().as_millis(),
            );
        }
        let now = now_secs();
        let graded: Vec<(ProbeStatus, String)> = self
            .certs
            .iter()
            .map(|(label, path)| {
                let (status, detail) =
                    grade_validity(agent_grpc::server::cert_file_validity(path), now);
                (status, format!("{label}: {detail}"))
            })
            .collect();
        ProbeOutcome::new(
            self.name(),
            worst(graded.iter().map(|(s, _)| *s)),
            graded
                .into_iter()
                .map(|(_, d)| d)
                .collect::<Vec<_>>()
                .join("; "),
            start.elapsed().as_millis(),
        )
    }
}

/// The most severe of several statuses (Fail > Warn > Ok > Skipped).
fn worst(statuses: impl Iterator<Item = ProbeStatus>) -> ProbeStatus {
    let rank = |s: ProbeStatus| match s {
        ProbeStatus::Skipped => 0,
        ProbeStatus::Ok => 1,
        ProbeStatus::Warn => 2,
        ProbeStatus::Fail => 3,
    };
    statuses
        .max_by_key(|s| rank(*s))
        .unwrap_or(ProbeStatus::Skipped)
}

#[cfg(test)]
mod tests;
