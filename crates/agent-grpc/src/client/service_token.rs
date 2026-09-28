//! The process's own service token, from its mTLS client certificate
//! (security-hardening S10, docs/design/security-hardening/04-service-integration.md).
//!
//! Work with no caller behind it (a fleet poll, a scheduled job) needs a credential
//! of its own when it calls another seam. [`MtlsBearerSource`] obtains one by calling
//! `AuthService.Exchange{use_client_cert}` over a mutual-TLS connection, presenting
//! `[grpc.tls.client]`'s certificate; the token that comes back is bound to that
//! certificate, so it is useless to anyone who copies it without the key.
//!
//! Installed once per process as the [`agent_core::BearerSource`], it is what
//! `outbound()` sends when no caller token is in scope (S9). A background task
//! exchanges again at two thirds of each token's lifetime, and retries a failed
//! exchange on the shared backoff schedule; [`BearerSource::bearer`] stops handing
//! out a token [`REFRESH_SKEW_SECS`] before it expires, so a call never leaves with
//! a token that dies in flight.

use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agent_core::{Bearer, BearerSource};
use agent_proto::pb;
use agent_retry::RetryPolicy;

use crate::tls::ClientTls;
use crate::transport::Endpoint;

/// A cached token is not handed out this close to its expiry.
pub const REFRESH_SKEW_SECS: u64 = 30;
/// Soonest the refresher exchanges again after a success.
pub const MIN_REFRESH_SECS: u64 = 5;
/// A token lifetime claimed by the server beyond this is capped (the agent never
/// issues longer ones), so a hostile or broken server cannot park the refresher.
pub const MAX_LIFETIME_SECS: u64 = 3600;

#[derive(Clone)]
struct Cached {
    bearer: Bearer,
    expires_at: u64,
}

/// Trades this process's client certificate for a service token and keeps it fresh.
pub struct MtlsBearerSource {
    endpoint: Endpoint,
    tls: Arc<ClientTls>,
    current: RwLock<Option<Cached>>,
}

impl MtlsBearerSource {
    /// A source that exchanges at `endpoint`, which must be `https://` (the
    /// exchange proves the certificate, so it must travel over TLS), presenting
    /// `tls`'s client certificate.
    pub fn new(endpoint: Endpoint, tls: Arc<ClientTls>) -> Result<Self, String> {
        if !matches!(endpoint, Endpoint::Tcp { tls: true, .. }) {
            return Err(format!(
                "the service-token endpoint {endpoint:?} must be an `https://` address: the \
                 exchange proves this process's client certificate"
            ));
        }
        if !tls.has_identity() {
            return Err(
                "a service token needs `[grpc.tls.client] cert` and `key`: it is \
                        traded for this process's client certificate"
                    .into(),
            );
        }
        Ok(Self {
            endpoint,
            tls,
            current: RwLock::new(None),
        })
    }

    /// Exchange once and cache the token. Returns how long to wait before the next
    /// exchange.
    pub async fn refresh(&self) -> Result<Duration, String> {
        let channel = self
            .endpoint
            .connect_lazy_with(Some(&self.tls))
            .map_err(|e| e.to_string())?;
        let resp = pb::auth_service_client::AuthServiceClient::new(channel)
            .exchange(pb::ExchangeRequest {
                use_client_cert: true,
                client_kind: "service".into(),
                ..Default::default()
            })
            .await
            .map_err(|s| format!("service-token exchange refused ({:?})", s.code()))?
            .into_inner();
        let now = now_secs();
        let lifetime = usable_lifetime(&resp.access_token, resp.expires_at, now)
            .ok_or("the exchange returned no usable token")?;
        let cached = Cached {
            bearer: Bearer::new(resp.access_token),
            expires_at: now + lifetime,
        };
        *self
            .current
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cached);
        Ok(next_refresh(lifetime))
    }

    /// Keep the token fresh for the life of the process: exchange now, then at two
    /// thirds of each lifetime; after a failure, back off (1 s doubling to 60 s)
    /// and try again. Never gives up: a seam coming up later is normal at startup.
    pub fn spawn_refresher(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let backoff = RetryPolicy::new(u32::MAX)
            .with_base_delay(Duration::from_secs(1))
            .with_max_delay(Duration::from_secs(60));
        // unscoped-spawn: the process's own credential; there is no caller to carry.
        tokio::spawn(async move {
            let mut failures: u32 = 0;
            loop {
                let wait = match self.refresh().await {
                    Ok(next) => {
                        failures = 0;
                        next
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "service token: exchange failed, retrying");
                        let wait = backoff.ceiling(failures);
                        failures = failures.saturating_add(1);
                        wait
                    }
                };
                tokio::time::sleep(wait).await;
            }
        })
    }

    /// The cached token if it is still good at `now`.
    fn bearer_at(&self, now: u64) -> Option<Bearer> {
        let current = self
            .current
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        current
            .as_ref()
            .filter(|c| now.saturating_add(REFRESH_SKEW_SECS) < c.expires_at)
            .map(|c| c.bearer.clone())
    }
}

impl BearerSource for MtlsBearerSource {
    fn bearer(&self) -> Option<Bearer> {
        self.bearer_at(now_secs())
    }
}

/// Seconds a returned token may be used for, or `None` when there is no token or it
/// would expire within the skew window. A lifetime beyond [`MAX_LIFETIME_SECS`] is
/// capped rather than trusted.
pub(crate) fn usable_lifetime(token: &str, expires_at: u64, now: u64) -> Option<u64> {
    let lifetime = expires_at.checked_sub(now)?.min(MAX_LIFETIME_SECS);
    (!token.is_empty() && lifetime > REFRESH_SKEW_SECS).then_some(lifetime)
}

/// Exchange again at two thirds of the lifetime, within bounds.
pub(crate) fn next_refresh(lifetime_secs: u64) -> Duration {
    Duration::from_secs((lifetime_secs * 2 / 3).clamp(MIN_REFRESH_SECS, MAX_LIFETIME_SECS))
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use agent_testkit::pki::{LeafSpec, TestPki};
    use rstest::rstest;

    use super::*;

    fn tls(with_cert: bool) -> Arc<ClientTls> {
        let pki = TestPki::new("service token CA");
        let leaf = pki.issue(&LeafSpec::service("fleet"));
        let identity = with_cert.then_some((leaf.cert_pem, leaf.key_pem));
        Arc::new(ClientTls::from_pem(Some(pki.ca_pem()), identity, None).expect("client tls"))
    }

    #[rstest]
    #[case::positive_https("https://127.0.0.1:50051", true, true)]
    #[case::negative_bare_hostport("127.0.0.1:50051", true, false)]
    #[case::negative_http("http://127.0.0.1:50051", true, false)]
    #[case::negative_unix_socket("unix:/tmp/agent-seddon/a.sock", true, false)]
    #[case::negative_no_client_cert("https://127.0.0.1:50051", false, false)]
    fn new_cases(#[case] endpoint: &str, #[case] with_cert: bool, #[case] ok: bool) {
        let got = MtlsBearerSource::new(Endpoint::parse(endpoint), tls(with_cert));
        assert_eq!(got.is_ok(), ok, "{:?}", got.err());
    }

    #[rstest]
    #[case::positive_normal("tok", 1_000 + 900, 1_000, Some(900))]
    #[case::boundary_just_over_skew("tok", 1_000 + REFRESH_SKEW_SECS + 1, 1_000, Some(REFRESH_SKEW_SECS + 1))]
    #[case::boundary_at_skew("tok", 1_000 + REFRESH_SKEW_SECS, 1_000, None)]
    #[case::negative_already_expired("tok", 999, 1_000, None)]
    #[case::negative_empty_token("", 1_000 + 900, 1_000, None)]
    #[case::adversarial_huge_expiry_capped("tok", u64::MAX, 1_000, Some(MAX_LIFETIME_SECS))]
    #[case::adversarial_zero_expiry("tok", 0, 1_000, None)]
    fn usable_lifetime_cases(
        #[case] token: &str,
        #[case] expires_at: u64,
        #[case] now: u64,
        #[case] want: Option<u64>,
    ) {
        assert_eq!(usable_lifetime(token, expires_at, now), want);
    }

    #[rstest]
    #[case::positive_two_thirds(900, 600)]
    #[case::boundary_floor(3, MIN_REFRESH_SECS)]
    #[case::boundary_cap(u64::MAX / 2, MAX_LIFETIME_SECS)]
    fn next_refresh_cases(#[case] lifetime: u64, #[case] want: u64) {
        assert_eq!(next_refresh(lifetime), Duration::from_secs(want));
    }

    #[rstest]
    #[case::corner_nothing_cached(None, 1_000, false)]
    #[case::positive_fresh(Some(2_000), 1_000, true)]
    #[case::boundary_inside_skew(Some(1_000 + REFRESH_SKEW_SECS), 1_000, false)]
    #[case::negative_expired(Some(900), 1_000, false)]
    fn bearer_at_cases(#[case] expires_at: Option<u64>, #[case] now: u64, #[case] want: bool) {
        let source =
            MtlsBearerSource::new(Endpoint::parse("https://127.0.0.1:1"), tls(true)).unwrap();
        *source.current.write().unwrap() = expires_at.map(|expires_at| Cached {
            bearer: Bearer::new("tok"),
            expires_at,
        });
        assert_eq!(source.bearer_at(now).is_some(), want);
    }
}
