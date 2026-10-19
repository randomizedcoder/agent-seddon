//! The GitHub App webhook receiver of `--serve-fleet` (forge-auth F6).
//!
//! `POST /webhooks/github` on `[review_fleet.webhook] listen`. Every delivery is
//! **attacker-reachable**, so the receiver [`screen`]s it before anything acts on it:
//! method + path, a body cap (read bounded, never buffered past [`MAX_WEBHOOK_BODY`]), the
//! `X-Hub-Signature-256` HMAC over the raw body against `[github_app] webhook_secret_ref`
//! (constant time, `agent_forge::webhook`), the `X-GitHub-Delivery` id's shape, and replay
//! (a delivery id seen in the last hour is acknowledged and dropped). Only then is the body
//! parsed ([`agent_review_fleet::webhook::parse`]) — typed and bounded — and dispatched to
//! the fleet off the HTTP thread, answering `202` at once (GitHub allows 10 s).
//!
//! What a delivery can do is narrow (see `agent_review_fleet::webhook`): trigger reviews
//! of the owning tenant's rows for a PR's repo, or narrow the `ghapp:` mint gate on
//! uninstall/suspend. It never binds an installation. Responses carry no detail beyond a
//! status, and neither the body nor the signature is ever logged.
//!
//! Limits: `tiny_http` has no per-connection read timeout, so a slow client holds one of
//! the [`MAX_IN_FLIGHT`] handler slots until it finishes or disconnects — serve it behind
//! the edge (Envoy), which buffers and times out requests. Delivery dedup is per process.

use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use agent_core::pending::SeenSet;
use agent_core::{
    FleetRegistry, FleetSession, FleetTrigger, ForgeRegistry, GithubInstallLifecycle, Result,
    TriggerOutcome, TriggerSink,
};
use agent_review_fleet::webhook::{self, Dispatched, WebhookTarget};
use async_trait::async_trait;

/// The one path the receiver serves.
pub(crate) const WEBHOOK_PATH: &str = "/webhooks/github";
/// Largest delivery body read. GitHub caps payloads at 25 MB, but the events the fleet
/// acts on are a few KB; anything bigger is refused, not buffered.
pub(crate) const MAX_WEBHOOK_BODY: usize = 1024 * 1024;
/// Longest `X-GitHub-Delivery` (a GUID) accepted.
pub(crate) const MAX_DELIVERY_ID_LEN: usize = 64;
/// How long a delivery id is remembered for replay detection, and how many.
const SEEN_TTL_SECS: u64 = 3600;
const SEEN_MAX: usize = 10_000;
/// Concurrent requests being read/screened; beyond it a delivery gets `503` (GitHub
/// redelivers).
pub(crate) const MAX_IN_FLIGHT: usize = 16;

/// One delivery as received — the parts [`screen`] decides on.
pub(crate) struct Incoming<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub signature: Option<&'a str>,
    pub event: Option<&'a str>,
    pub delivery: Option<&'a str>,
    /// The body read so far, at most `MAX_WEBHOOK_BODY + 1` bytes.
    pub body: &'a [u8],
}

/// What [`screen`] decided.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Screened {
    /// Authentic and new: act on it.
    Accept { event: String, delivery: String },
    /// Authentic but already handled: acknowledge, do nothing.
    Replay,
    /// Refused with this status; the reason is for the log only.
    Refuse(u16, &'static str),
}

fn valid_delivery(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_DELIVERY_ID_LEN
        && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

fn valid_event(event: &str) -> bool {
    !event.is_empty()
        && event.len() <= 64
        && event.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

/// Decide on a delivery. Order matters: everything checkable without the secret first
/// (cheap refusals), then the HMAC, and only an authentic delivery may spend a replay slot
/// (an unsigned flood can't evict real delivery ids).
pub(crate) fn screen(req: &Incoming<'_>, secret: &[u8], seen: &SeenSet, now: u64) -> Screened {
    let path = req.url.split('?').next().unwrap_or_default();
    if path != WEBHOOK_PATH {
        return Screened::Refuse(404, "unknown path");
    }
    if req.method != "POST" {
        return Screened::Refuse(405, "method not allowed");
    }
    if req.body.len() > MAX_WEBHOOK_BODY {
        return Screened::Refuse(413, "body over the cap");
    }
    let Some(signature) = req.signature else {
        return Screened::Refuse(401, "unsigned delivery");
    };
    if !agent_runtime::verify_webhook_signature(secret, req.body, signature) {
        return Screened::Refuse(401, "bad signature");
    }
    let Some(event) = req.event.filter(|e| valid_event(e)) else {
        return Screened::Refuse(400, "missing or malformed event");
    };
    let Some(delivery) = req.delivery.filter(|d| valid_delivery(d)) else {
        return Screened::Refuse(400, "missing or malformed delivery id");
    };
    if !seen.first_sight(now, delivery) {
        return Screened::Replay;
    }
    Screened::Accept {
        event: event.to_string(),
        delivery: delivery.to_string(),
    }
}

/// The fleet as a [`WebhookTarget`]: the roster (scoped per tenant), the forge cards, the
/// trigger queue, and — for a per-tenant fleet — the verified-install lifecycle.
pub(crate) struct FleetWebhookTarget {
    pub per_tenant: bool,
    pub lifecycle: Option<Arc<dyn GithubInstallLifecycle>>,
    pub roster: Arc<dyn FleetRegistry>,
    pub forge_registry: Option<Arc<dyn ForgeRegistry>>,
    pub sink: Arc<dyn TriggerSink>,
}

impl FleetWebhookTarget {
    /// Whether a row reaches its forge through GitHub: inline `backend = "github"`, or a
    /// forge card (read in the tenant's scope) of kind `github`. A missing card ⇒ no.
    async fn is_github(&self, row: &FleetSession) -> bool {
        if row.forge_id.is_empty() {
            return row.backend == "github";
        }
        match &self.forge_registry {
            Some(reg) => reg
                .get(&row.forge_id)
                .await
                .is_ok_and(|card| card.enabled && card.kind == "github"),
            None => false,
        }
    }

    /// The single-operator fleet owns no verified-install store: an uninstall or suspend
    /// can only drop the cached token (GitHub refuses the next mint).
    fn forget_token(installation_id: &str) {
        if let Some(m) = agent_runtime::secrets::minter() {
            m.forget(installation_id);
        }
    }
}

#[async_trait]
impl WebhookTarget for FleetWebhookTarget {
    async fn owner_of(&self, installation: &str) -> Result<Option<String>> {
        if !self.per_tenant {
            return Ok(Some(String::new()));
        }
        match &self.lifecycle {
            Some(l) => l.owner_of(installation).await,
            None => Ok(None),
        }
    }

    async fn github_rows(&self, tenant: &str) -> Result<Vec<FleetSession>> {
        let rows = async {
            let mut out = Vec::new();
            for row in self.roster.list().await? {
                if row.enabled && self.is_github(&row).await {
                    out.push(row);
                }
            }
            Ok(out)
        };
        agent_core::in_tenant(tenant, rows)
            .await
            .unwrap_or_else(|| Err(agent_core::Error::Fleet("invalid tenant".into())))
    }

    fn enqueue(&self, trigger: FleetTrigger) -> TriggerOutcome {
        self.sink.enqueue(trigger)
    }

    async fn revoke(&self, tenant: &str, installation: &str) -> Result<()> {
        match (&self.lifecycle, tenant.is_empty()) {
            (Some(l), false) => l.revoke(tenant, installation).await,
            _ => {
                Self::forget_token(installation);
                Ok(())
            }
        }
    }

    async fn suspend(&self, tenant: &str, installation: &str) -> Result<()> {
        match (&self.lifecycle, tenant.is_empty()) {
            (Some(l), false) => l.suspend(tenant, installation).await,
            _ => {
                Self::forget_token(installation);
                Ok(())
            }
        }
    }

    async fn unsuspend(&self, tenant: &str, installation: &str) -> Result<()> {
        match (&self.lifecycle, tenant.is_empty()) {
            (Some(l), false) => l.unsuspend(tenant, installation).await,
            _ => Ok(()),
        }
    }

    async fn sync_repos(
        &self,
        tenant: &str,
        installation: &str,
        added: &[String],
        removed: &[String],
    ) -> Result<()> {
        match (&self.lifecycle, tenant.is_empty()) {
            (Some(l), false) => l.sync_repos(tenant, installation, added, removed).await,
            _ => Ok(()),
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn header<'r>(req: &'r tiny_http::Request, name: &'static str) -> Option<&'r str> {
    req.headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str())
}

fn respond(req: tiny_http::Request, status: u16) {
    let _ = req.respond(tiny_http::Response::empty(status));
}

/// Serve deliveries on `server` until it is dropped. Each request is screened on its own
/// thread (at most [`MAX_IN_FLIGHT`]); an accepted one is parsed and dispatched on
/// `runtime`.
pub(crate) fn serve(
    server: tiny_http::Server,
    secret: agent_core::Secret,
    target: Arc<dyn WebhookTarget>,
    runtime: tokio::runtime::Handle,
) -> std::thread::JoinHandle<()> {
    let secret = Arc::new(secret);
    let seen = Arc::new(SeenSet::new(SEEN_TTL_SECS, SEEN_MAX));
    let in_flight = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            if in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
                in_flight.fetch_sub(1, Ordering::SeqCst);
                respond(req, 503);
                continue;
            }
            let (secret, seen, target, runtime, in_flight) = (
                secret.clone(),
                seen.clone(),
                target.clone(),
                runtime.clone(),
                in_flight.clone(),
            );
            std::thread::spawn(move || {
                handle(req, &secret, &seen, target, &runtime);
                in_flight.fetch_sub(1, Ordering::SeqCst);
            });
        }
    })
}

fn handle(
    mut req: tiny_http::Request,
    secret: &agent_core::Secret,
    seen: &SeenSet,
    target: Arc<dyn WebhookTarget>,
    runtime: &tokio::runtime::Handle,
) {
    // A declared length over the cap is refused before reading anything.
    if req.body_length().is_some_and(|n| n > MAX_WEBHOOK_BODY) {
        return respond(req, 413);
    }
    let mut body = Vec::new();
    let cap = (MAX_WEBHOOK_BODY + 1) as u64;
    if req.as_reader().take(cap).read_to_end(&mut body).is_err() {
        return respond(req, 400);
    }
    let method = req.method().as_str().to_string();
    let url = req.url().to_string();
    let decision = screen(
        &Incoming {
            method: &method,
            url: &url,
            signature: header(&req, "X-Hub-Signature-256"),
            event: header(&req, "X-GitHub-Event"),
            delivery: header(&req, "X-GitHub-Delivery"),
            body: &body,
        },
        secret.expose().as_bytes(),
        seen,
        now_secs(),
    );
    let (event, delivery) = match decision {
        Screened::Accept { event, delivery } => (event, delivery),
        Screened::Replay => {
            tracing::info!("fleet webhook: replayed delivery acknowledged, not re-run");
            return respond(req, 200);
        }
        Screened::Refuse(status, why) => {
            tracing::warn!(status, reason = why, "fleet webhook: delivery refused");
            return respond(req, status);
        }
    };
    let parsed = match webhook::parse(&event, &body) {
        Ok(e) => e,
        Err(webhook::Reject(why)) => {
            tracing::warn!(%event, %delivery, reason = why, "fleet webhook: payload refused");
            return respond(req, 400);
        }
    };
    respond(req, 202);
    // unscoped-spawn: a webhook has no caller identity; `dispatch` scopes each tenant's
    // work itself (the owning tenant comes from the verified install claim).
    runtime.spawn(async move {
        match webhook::dispatch(parsed, target.as_ref()).await {
            Ok(Dispatched::Unclaimed) => {
                tracing::info!(%event, %delivery, "fleet webhook: installation not claimed by any tenant; ignored");
            }
            Ok(outcome) => tracing::info!(%event, %delivery, ?outcome, "fleet webhook: handled"),
            Err(e) => tracing::warn!(%event, %delivery, error = %e, "fleet webhook: dispatch failed"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::sync::Mutex;

    const SECRET: &[u8] = b"It's a Secret to Everybody";
    const BODY: &[u8] = b"Hello, World!";
    // GitHub's documented test vector for SECRET over BODY.
    const SIG: &str = "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17";
    const DELIVERY: &str = "72d3162e-cc78-11e3-81ab-4c9367dc0958";

    fn req<'a>(
        method: &'a str,
        url: &'a str,
        sig: Option<&'a str>,
        event: Option<&'a str>,
        delivery: Option<&'a str>,
        body: &'a [u8],
    ) -> Incoming<'a> {
        Incoming {
            method,
            url,
            signature: sig,
            event,
            delivery,
            body,
        }
    }

    fn ok() -> Incoming<'static> {
        req(
            "POST",
            WEBHOOK_PATH,
            Some(SIG),
            Some("ping"),
            Some(DELIVERY),
            BODY,
        )
    }

    fn accept() -> Screened {
        Screened::Accept {
            event: "ping".into(),
            delivery: DELIVERY.into(),
        }
    }

    #[rstest]
    #[case::positive_signed_delivery(ok(), accept())]
    #[case::corner_query_string_ignored(Incoming { url: "/webhooks/github?x=1", ..ok() }, accept())]
    #[case::negative_unknown_path(Incoming { url: "/webhooks/gitlab", ..ok() }, Screened::Refuse(404, "unknown path"))]
    #[case::negative_get(Incoming { method: "GET", ..ok() }, Screened::Refuse(405, "method not allowed"))]
    #[case::negative_unsigned(Incoming { signature: None, ..ok() }, Screened::Refuse(401, "unsigned delivery"))]
    #[case::negative_missing_event(Incoming { event: None, ..ok() }, Screened::Refuse(400, "missing or malformed event"))]
    #[case::negative_missing_delivery(Incoming { delivery: None, ..ok() }, Screened::Refuse(400, "missing or malformed delivery id"))]
    #[case::adversarial_wrong_signature(Incoming { signature: Some("sha256=0000000000000000000000000000000000000000000000000000000000000000"), ..ok() }, Screened::Refuse(401, "bad signature"))]
    #[case::adversarial_sha1_signature(Incoming { signature: Some("sha1=757107ea0eb2509fc211221cce984b8a37570b6d"), ..ok() }, Screened::Refuse(401, "bad signature"))]
    #[case::adversarial_tampered_body(Incoming { body: b"Hello, World?", ..ok() }, Screened::Refuse(401, "bad signature"))]
    #[case::adversarial_path_traversal(Incoming { url: "/webhooks/github/../admin", ..ok() }, Screened::Refuse(404, "unknown path"))]
    #[case::adversarial_crlf_delivery(Incoming { delivery: Some("72d3\r\nX-Evil: 1"), ..ok() }, Screened::Refuse(400, "missing or malformed delivery id"))]
    #[case::adversarial_event_injection(Incoming { event: Some("ping\nx"), ..ok() }, Screened::Refuse(400, "missing or malformed event"))]
    #[case::boundary_delivery_at_cap(Incoming { delivery: Some(&"a".repeat(MAX_DELIVERY_ID_LEN)), ..ok() }, Screened::Accept { event: "ping".into(), delivery: "a".repeat(MAX_DELIVERY_ID_LEN) })]
    #[case::boundary_delivery_over_cap(Incoming { delivery: Some(&"a".repeat(MAX_DELIVERY_ID_LEN + 1)), ..ok() }, Screened::Refuse(400, "missing or malformed delivery id"))]
    fn screen_cases(#[case] incoming: Incoming<'_>, #[case] want: Screened) {
        let seen = SeenSet::new(SEEN_TTL_SECS, 16);
        assert_eq!(screen(&incoming, SECRET, &seen, 1_000), want);
    }

    #[test]
    fn boundary_body_over_cap_refused_before_hmac() {
        let big = vec![b'x'; MAX_WEBHOOK_BODY + 1];
        let seen = SeenSet::new(SEEN_TTL_SECS, 16);
        let got = screen(&Incoming { body: &big, ..ok() }, SECRET, &seen, 1_000);
        assert_eq!(got, Screened::Refuse(413, "body over the cap"));
    }

    #[test]
    fn negative_empty_secret_never_authenticates() {
        let seen = SeenSet::new(SEEN_TTL_SECS, 16);
        assert_eq!(
            screen(&ok(), b"", &seen, 1_000),
            Screened::Refuse(401, "bad signature")
        );
    }

    #[test]
    fn adversarial_replay_acknowledged_once_and_unsigned_cannot_spend_slots() {
        let seen = SeenSet::new(SEEN_TTL_SECS, 16);
        // An unsigned delivery with the same id is refused and records nothing…
        let forged = Incoming {
            signature: None,
            ..ok()
        };
        assert_eq!(
            screen(&forged, SECRET, &seen, 1_000),
            Screened::Refuse(401, "unsigned delivery")
        );
        assert!(seen.is_empty());
        // …so the real one is still new, and its redelivery is a replay.
        assert_eq!(screen(&ok(), SECRET, &seen, 1_000), accept());
        assert_eq!(screen(&ok(), SECRET, &seen, 1_000), Screened::Replay);
    }

    #[derive(Default)]
    struct Recorder {
        triggers: Mutex<Vec<FleetTrigger>>,
    }

    #[async_trait]
    impl WebhookTarget for Recorder {
        async fn owner_of(&self, _i: &str) -> Result<Option<String>> {
            Ok(Some("acme".into()))
        }
        async fn github_rows(&self, _t: &str) -> Result<Vec<FleetSession>> {
            Ok(vec![FleetSession {
                id: "web".into(),
                repo: "acme__web".into(),
                enabled: true,
                ..Default::default()
            }])
        }
        fn enqueue(&self, trigger: FleetTrigger) -> TriggerOutcome {
            self.triggers.lock().unwrap().push(trigger);
            TriggerOutcome::Accepted
        }
        async fn revoke(&self, _t: &str, _i: &str) -> Result<()> {
            Ok(())
        }
        async fn suspend(&self, _t: &str, _i: &str) -> Result<()> {
            Ok(())
        }
        async fn unsuspend(&self, _t: &str, _i: &str) -> Result<()> {
            Ok(())
        }
        async fn sync_repos(&self, _t: &str, _i: &str, _a: &[String], _r: &[String]) -> Result<()> {
            Ok(())
        }
    }

    fn sign(body: &[u8]) -> String {
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, SECRET);
        let tag = ring::hmac::sign(&key, body);
        let hex: String = tag.as_ref().iter().map(|b| format!("{b:02x}")).collect();
        format!("sha256={hex}")
    }

    /// The receiver end to end over a real socket: a signed `pull_request` is accepted
    /// (`202`) and becomes a trigger for the owning tenant; its redelivery is `200` and
    /// enqueues nothing; a forged one is `401`; an oversized one is `413`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn positive_socket_round_trip() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
        let addr = server.server_addr().to_ip().expect("ip addr");
        let target = Arc::new(Recorder::default());
        let _h = serve(
            server,
            agent_core::Secret::new(std::str::from_utf8(SECRET).unwrap()),
            target.clone(),
            tokio::runtime::Handle::current(),
        );
        let url = format!("http://{addr}{WEBHOOK_PATH}");
        let body = serde_json::json!({
            "action": "synchronize", "number": 7,
            "pull_request": { "draft": false },
            "repository": { "full_name": "acme/web" },
            "installation": { "id": 42 },
        })
        .to_string();
        let client = reqwest::Client::new();
        let post = |body: String, sig: String, delivery: &str| {
            client
                .post(&url)
                .header("X-GitHub-Event", "pull_request")
                .header("X-GitHub-Delivery", delivery)
                .header("X-Hub-Signature-256", sig)
                .body(body)
                .send()
        };
        let status = |r: reqwest::Response| r.status().as_u16();

        assert_eq!(
            status(
                post(body.clone(), sign(body.as_bytes()), "d-1")
                    .await
                    .unwrap()
            ),
            202
        );
        assert_eq!(
            status(
                post(body.clone(), sign(body.as_bytes()), "d-1")
                    .await
                    .unwrap()
            ),
            200
        );
        assert_eq!(
            status(post(body.clone(), sign(b"other"), "d-2").await.unwrap()),
            401
        );
        let big = "x".repeat(MAX_WEBHOOK_BODY + 1);
        assert_eq!(
            status(
                post(big.clone(), sign(big.as_bytes()), "d-3")
                    .await
                    .unwrap()
            ),
            413
        );

        for _ in 0..50 {
            if !target.triggers.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let got = target.triggers.lock().unwrap().clone();
        assert_eq!(
            got.len(),
            1,
            "one trigger, the replay and refusals enqueue nothing"
        );
        assert_eq!(
            (
                got[0].tenant.as_str(),
                got[0].session_id.as_str(),
                got[0].pr_number
            ),
            ("acme", "web", 7)
        );
    }
}
