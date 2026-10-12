//! Reload a serve mode's TLS material and token signing keys without a restart:
//! on SIGHUP (security-hardening S20) and, with `[grpc.tls] reload_poll_secs`, when
//! the files change (K4).
//!
//! What reloads: the listener's `[grpc.tls]` certificate, key and client CA
//! ([`agent_grpc::ServerTls::reload`]), the `[grpc.tls.client]` CA and client
//! certificate every `https://` dial uses ([`agent_grpc::ClientTls::reload`], S20b),
//! and the `[auth.token]` `signing_key` / `previous_key`
//! ([`agent_grpc::server::AuthLayer::reload_keys`]). Each part is
//! reloaded on its own, and a part that fails keeps what it had: a half-written
//! renewal logs a warning and the listener keeps serving the old certificate.
//! Each part's outcome counts `agent_tls_reload_total{part, trigger, result}`.
//!
//! Pair SIGHUP with the renewer, for example
//! `step ca renew --daemon --exec "kill -HUP <pid>" server.crt server.key`.
//! A Kubernetes Secret volume sends no signal: the kubelet writes a new timestamped
//! directory and swaps the `..data` symlink. There the poll is the trigger: it hashes
//! the configured paths (following the symlink, as the loaders do) and reloads on
//! any difference. A torn read fails the reload (`rustls` refuses a key that does not
//! match its certificate), so the old material stays and the next tick retries.

use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use agent_grpc::server::AuthLayer;
use agent_grpc::{ClientTls, ServerTls};

/// What one part of a reload did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Reloaded {
    /// `tls`, `client_tls` or `signing_key`.
    pub what: &'static str,
    /// What is in use now, or why the old material was kept.
    pub result: Result<String, String>,
}

/// What started a reload: the `trigger` label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trigger {
    Sighup,
    Poll,
}

impl Trigger {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sighup => "sighup",
            Self::Poll => "poll",
        }
    }
}

/// Counts one reloaded part as `(part, trigger, result)`, `result` being `ok` or
/// `error`. The serve path bridges it to `agent_tls_reload_total`.
pub(crate) type ReloadRecorder = Arc<dyn Fn(&str, &str, &str) + Send + Sync>;

/// Reload every part this serve mode holds, each independently.
pub(crate) fn reload_all(
    tls: Option<&ServerTls>,
    client: Option<&ClientTls>,
    auth: &AuthLayer,
) -> Vec<Reloaded> {
    let mut out = Vec::new();
    if let Some(tls) = tls {
        let now = if tls.is_mutual() {
            "certificate, key and client CA"
        } else {
            "certificate and key"
        };
        out.push(Reloaded {
            what: "tls",
            result: tls.reload().map(|()| now.to_string()),
        });
    }
    if let Some(client) = client {
        let now = if client.has_identity() {
            "trust anchors and client certificate"
        } else {
            "trust anchors"
        };
        out.push(Reloaded {
            what: "client_tls",
            result: client.reload().map(|()| now.to_string()),
        });
    }
    #[cfg(feature = "auth")]
    if let Some(keys) = auth.reload_keys() {
        out.push(Reloaded {
            what: "signing_key",
            result: keys.map(|k| match k.previous_kid {
                Some(previous) => format!("kid {} (previous {previous})", k.current_kid),
                None => format!("kid {}", k.current_kid),
            }),
        });
    }
    #[cfg(not(feature = "auth"))]
    let _ = auth;
    out
}

fn log(outcomes: &[Reloaded], trigger: Trigger) {
    let trigger = trigger.as_str();
    if outcomes.is_empty() {
        tracing::info!(
            trigger,
            "nothing to reload (no `[grpc.tls]` cert, no `[grpc.tls.client]`, \
             no `[auth.token]`)"
        );
    }
    for o in outcomes {
        match &o.result {
            Ok(now) => tracing::info!(what = o.what, now = %now, trigger, "reloaded"),
            Err(e) => tracing::warn!(
                what = o.what,
                error = %e,
                trigger,
                "reload failed; still using the previous material"
            ),
        }
    }
}

/// The parts a serve mode reloads, and where the outcome is counted.
#[derive(Clone)]
pub(crate) struct Reloader {
    tls: Option<ServerTls>,
    auth: AuthLayer,
    /// The dialing TLS current now. Read per reload: a config reload may have
    /// installed a new one.
    client: Arc<dyn Fn() -> Option<Arc<ClientTls>> + Send + Sync>,
    record: ReloadRecorder,
}

impl Reloader {
    /// The serve path's reloader: the process-wide dialing TLS
    /// ([`agent_grpc::tls::client_tls`]).
    pub(crate) fn new(tls: Option<ServerTls>, auth: AuthLayer, record: ReloadRecorder) -> Self {
        Self {
            tls,
            auth,
            client: Arc::new(agent_grpc::tls::client_tls),
            record,
        }
    }

    /// Reload every part, log and count each outcome.
    fn reload(&self, trigger: Trigger) -> Vec<Reloaded> {
        let client = (self.client)();
        let out = reload_all(self.tls.as_ref(), client.as_deref(), &self.auth);
        log(&out, trigger);
        for o in &out {
            let result = if o.result.is_ok() { "ok" } else { "error" };
            (self.record)(o.what, trigger.as_str(), result);
        }
        out
    }

    /// Every file a reload reads, in a stable order.
    fn files(&self) -> Vec<PathBuf> {
        let mut files = self.tls.as_ref().map_or_else(Vec::new, ServerTls::files);
        if let Some(client) = (self.client)() {
            files.extend(client.files());
        }
        #[cfg(feature = "auth")]
        files.extend(self.auth.key_files());
        files
    }
}

/// What the watched files held at one tick: per path, a hash of its first
/// [`agent_grpc::tls::MAX_PEM_BYTES`] + 1 bytes (enough to see any change a loader
/// would accept, and never more buffered than a loader would read), or `None` when
/// it could not be read (missing mid-swap, say). A change detector, not a security
/// boundary: the reload still validates everything it loads.
#[derive(Debug, PartialEq, Eq)]
struct Fingerprint(Vec<(PathBuf, Option<u64>)>);

impl Fingerprint {
    fn of(files: Vec<PathBuf>) -> Self {
        Self(
            files
                .into_iter()
                .map(|path| {
                    let hash = hash_capped(&path);
                    (path, hash)
                })
                .collect(),
        )
    }
}

fn hash_capped(path: &std::path::Path) -> Option<u64> {
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    file.take(agent_grpc::tls::MAX_PEM_BYTES + 1)
        .read_to_end(&mut buf)
        .ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    buf.hash(&mut h);
    Some(h.finish())
}

/// The `reload_poll_secs` loop's state: the files as of the last reload that
/// succeeded (or of startup, when the loaders read them).
pub(crate) struct Poller {
    reloader: Reloader,
    seen: Fingerprint,
}

impl Poller {
    fn new(reloader: Reloader) -> Self {
        let seen = Fingerprint::of(reloader.files());
        Self { reloader, seen }
    }

    /// One tick: when the files differ from what was last loaded, reload once.
    /// The new state is kept only if every part reloaded, so a torn read or a
    /// renewal still being written is retried on the next tick (a part that stays
    /// broken warns each tick until it is fixed). `None` when nothing changed.
    fn tick(&mut self) -> Option<Vec<Reloaded>> {
        let now = Fingerprint::of(self.reloader.files());
        if now == self.seen {
            return None;
        }
        let out = self.reloader.reload(Trigger::Poll);
        if out.iter().all(|o| o.result.is_ok()) {
            self.seen = now;
        }
        Some(out)
    }
}

/// Reload on every SIGHUP for the life of the process. Installing the handler also
/// means a SIGHUP no longer terminates a serve mode (its default action), with or
/// without anything to reload.
pub(crate) fn watch_sighup(reloader: Reloader) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut hup = match signal(SignalKind::hangup()) {
            Ok(hup) => hup,
            Err(e) => {
                tracing::warn!(
                    "cannot watch SIGHUP ({e}); TLS and signing keys reload only on restart"
                );
                return;
            }
        };
        tokio::spawn(async move {
            while hup.recv().await.is_some() {
                reloader.reload(Trigger::Sighup);
            }
        });
    }
    #[cfg(not(unix))]
    let _ = reloader;
}

/// With `[grpc.tls] reload_poll_secs` > 0, check the files every that many seconds
/// for the life of the process ([`Poller::tick`]). `0` spawns nothing and returns
/// `None`: SIGHUP stays the only trigger. A SIGHUP reload is seen by the next tick
/// as a change and reloaded once more, which is harmless.
pub(crate) fn watch_poll(secs: u64, reloader: Reloader) -> Option<tokio::task::JoinHandle<()>> {
    if secs == 0 {
        return None;
    }
    let mut poller = Poller::new(reloader);
    tracing::info!(
        every_secs = secs,
        "reloading TLS and signing keys on file change"
    );
    Some(tokio::spawn(async move {
        let period = std::time::Duration::from_secs(secs);
        let mut every = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            every.tick().await;
            // File reads: off the runtime's worker threads.
            poller = match tokio::task::spawn_blocking(move || {
                poller.tick();
                poller
            })
            .await
            {
                Ok(poller) => poller,
                Err(e) => {
                    tracing::warn!("TLS reload poll stopped: {e}");
                    return;
                }
            };
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::pki::{LeafSpec, TestPki};

    /// A listener TLS loaded from files in a fresh dir; returns it and the cert path.
    fn tls_from_files(mutual: bool) -> (ServerTls, std::path::PathBuf) {
        let pki = TestPki::new("ca");
        let dir = agent_testkit::tempdir();
        let (cert, key) = pki.issue(&LeafSpec::service("seam")).write_to(&dir, "s");
        let ca = pki.write_ca(&dir, "ca");
        let tls = ServerTls::load(&cert, &key, mutual.then_some(ca.as_path())).unwrap();
        (tls, cert)
    }

    #[test]
    fn positive_tls_reload_is_reported() {
        let (tls, _) = tls_from_files(true);
        let got = reload_all(Some(&tls), None, &AuthLayer::disabled());
        assert_eq!(
            got,
            vec![Reloaded {
                what: "tls",
                result: Ok("certificate, key and client CA".into()),
            }]
        );
    }

    #[test]
    fn negative_broken_cert_is_reported_not_raised() {
        let (tls, cert) = tls_from_files(false);
        std::fs::write(&cert, "-----BEGIN CERTIFICATE-----\nAA==\n").unwrap();
        let got = reload_all(Some(&tls), None, &AuthLayer::disabled());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].what, "tls");
        assert!(got[0].result.is_err(), "{got:?}");
    }

    #[test]
    fn adversarial_oversized_cert_is_refused_and_reported() {
        let (tls, cert) = tls_from_files(false);
        let mut huge = b"-----BEGIN CERTIFICATE-----\n".to_vec();
        huge.resize(agent_grpc::tls::MAX_PEM_BYTES as usize + 1, b'A');
        std::fs::write(&cert, huge).unwrap();
        let got = reload_all(Some(&tls), None, &AuthLayer::disabled());
        let e = got[0].result.as_ref().unwrap_err();
        assert!(e.contains("exceeds"), "{e}");
    }

    #[test]
    fn corner_nothing_configured_reloads_nothing() {
        assert!(reload_all(None, None, &AuthLayer::disabled()).is_empty());
    }

    #[test]
    fn boundary_plain_tls_names_no_client_ca() {
        let (tls, _) = tls_from_files(false);
        let got = reload_all(Some(&tls), None, &AuthLayer::disabled());
        assert_eq!(got[0].result, Ok("certificate and key".into()));
    }

    /// A dialing TLS loaded from files; returns it and the CA path.
    fn client_from_files(identity: bool) -> (ClientTls, std::path::PathBuf) {
        let pki = TestPki::new("ca");
        let dir = agent_testkit::tempdir();
        let (cert, key) = pki.issue(&LeafSpec::service("c")).write_to(&dir, "c");
        let ca = pki.write_ca(&dir, "ca");
        let tls = ClientTls::load(
            Some(&ca),
            identity.then_some(cert.as_path()),
            identity.then_some(key.as_path()),
            None,
        )
        .unwrap();
        (tls, ca)
    }

    #[test]
    fn positive_client_tls_reload_is_reported_beside_the_listener() {
        let (tls, _) = tls_from_files(false);
        let (client, _) = client_from_files(true);
        let got = reload_all(Some(&tls), Some(&client), &AuthLayer::disabled());
        let what: Vec<_> = got.iter().map(|r| r.what).collect();
        assert_eq!(what, ["tls", "client_tls"]);
        assert_eq!(
            got[1].result,
            Ok("trust anchors and client certificate".into())
        );
    }

    #[test]
    fn negative_broken_client_ca_does_not_stop_the_listener_reload() {
        // Each part reloads on its own: a bad client CA is reported, and the
        // listener's reload still happens.
        let (tls, _) = tls_from_files(false);
        let (client, ca) = client_from_files(false);
        std::fs::write(&ca, "not a CA").unwrap();
        let got = reload_all(Some(&tls), Some(&client), &AuthLayer::disabled());
        assert!(got[0].result.is_ok(), "{got:?}");
        assert_eq!(got[1].what, "client_tls");
        assert!(got[1].result.is_err(), "{got:?}");
    }

    #[test]
    fn adversarial_in_memory_client_tls_reports_nothing_to_reload() {
        let got = reload_all(None, Some(&ClientTls::default()), &AuthLayer::disabled());
        let e = got[0].result.as_ref().unwrap_err();
        assert!(e.contains("nothing to reload"), "{e}");
    }

    #[test]
    fn boundary_client_without_identity_names_only_trust_anchors() {
        let (client, _) = client_from_files(false);
        let got = reload_all(None, Some(&client), &AuthLayer::disabled());
        assert_eq!(got[0].result, Ok("trust anchors".into()));
    }

    // ---- K4: reload on file change ------------------------------------------------

    type Counted = Arc<std::sync::Mutex<Vec<(String, String, String)>>>;

    /// A reloader over `tls` and `client` that records each counted outcome.
    fn reloader(tls: Option<ServerTls>, client: Option<ClientTls>) -> (Reloader, Counted) {
        let counted = Counted::default();
        let sink = counted.clone();
        let client = client.map(Arc::new);
        let r = Reloader {
            tls,
            auth: AuthLayer::disabled(),
            client: Arc::new(move || client.clone()),
            record: Arc::new(move |part: &str, trigger: &str, result: &str| {
                sink.lock()
                    .unwrap()
                    .push((part.into(), trigger.into(), result.into()));
            }),
        };
        (r, counted)
    }

    fn counted(c: &Counted) -> Vec<(String, String, String)> {
        c.lock().unwrap().clone()
    }

    fn row(part: &str, trigger: &str, result: &str) -> (String, String, String) {
        (part.into(), trigger.into(), result.into())
    }

    /// What each tick returned: `None` (no change) or each part's `Ok`/`Err`.
    fn ticks(p: &mut Poller, n: usize) -> Vec<Option<Vec<bool>>> {
        (0..n)
            .map(|_| {
                p.tick()
                    .map(|out| out.iter().map(|o| o.result.is_ok()).collect())
            })
            .collect()
    }

    /// A Secret volume laid out the way the kubelet writes one: each generation in
    /// `..<n>/`, `..data` pointing at the current one, and every name a symlink
    /// through `..data`. A rotation writes a new generation and swaps `..data` by
    /// rename, which is atomic.
    #[cfg(unix)]
    struct SecretDir {
        dir: std::path::PathBuf,
        generation: u32,
    }

    #[cfg(unix)]
    impl SecretDir {
        fn new(pki: &TestPki) -> Self {
            let mut s = Self {
                dir: agent_testkit::tempdir(),
                generation: 0,
            };
            s.rotate(pki);
            for name in ["tls.crt", "tls.key", "ca.crt"] {
                std::os::unix::fs::symlink(format!("..data/{name}"), s.path(name)).unwrap();
            }
            s
        }

        fn path(&self, name: &str) -> std::path::PathBuf {
            self.dir.join(name)
        }

        /// Write a new leaf (and `pki`'s CA) as the next generation and swap it in.
        fn rotate(&mut self, pki: &TestPki) {
            self.generation += 1;
            let generation = format!("..{}", self.generation);
            let gen_dir = self.path(&generation);
            std::fs::create_dir(&gen_dir).unwrap();
            pki.issue(&LeafSpec::service("seam"))
                .write_to(&gen_dir, "tls");
            pki.write_ca(&gen_dir, "ca");
            let tmp = self.path("..data_tmp");
            std::os::unix::fs::symlink(&generation, &tmp).unwrap();
            std::fs::rename(&tmp, self.path("..data")).unwrap();
        }

        fn load(&self) -> ServerTls {
            ServerTls::load(
                &self.path("tls.crt"),
                &self.path("tls.key"),
                Some(&self.path("ca.crt")),
            )
            .unwrap()
        }
    }

    #[cfg(unix)]
    #[test]
    fn positive_data_symlink_swap_reloads_once() {
        let pki = TestPki::new("ca");
        let mut secret = SecretDir::new(&pki);
        let (r, c) = reloader(Some(secret.load()), None);
        let mut p = Poller::new(r);
        assert_eq!(ticks(&mut p, 1), [None], "nothing changed since load");
        secret.rotate(&pki);
        assert_eq!(ticks(&mut p, 2), [Some(vec![true]), None]);
        assert_eq!(counted(&c), [row("tls", "poll", "ok")]);
    }

    #[test]
    fn negative_cert_rotated_without_its_key_keeps_the_old_pair_and_retries() {
        let pki = TestPki::new("ca");
        let dir = agent_testkit::tempdir();
        let (cert, key) = pki.issue(&LeafSpec::service("seam")).write_to(&dir, "s");
        let (r, c) = reloader(Some(ServerTls::load(&cert, &key, None).unwrap()), None);
        let mut p = Poller::new(r);
        let renewed = pki.issue(&LeafSpec::service("seam"));
        std::fs::write(&cert, &renewed.cert_pem).unwrap();
        // The key no longer matches: refused, and retried on the next tick.
        assert_eq!(ticks(&mut p, 2), [Some(vec![false]), Some(vec![false])]);
        std::fs::write(&key, &renewed.key_pem).unwrap();
        assert_eq!(ticks(&mut p, 2), [Some(vec![true]), None]);
        assert_eq!(
            counted(&c),
            [
                row("tls", "poll", "error"),
                row("tls", "poll", "error"),
                row("tls", "poll", "ok")
            ]
        );
    }

    #[test]
    fn positive_client_tls_change_is_polled() {
        let (client, ca) = client_from_files(false);
        let (r, c) = reloader(None, Some(client));
        let mut p = Poller::new(r);
        TestPki::new("rotated-ca").write_ca(ca.parent().unwrap(), "ca");
        assert_eq!(ticks(&mut p, 2), [Some(vec![true]), None]);
        assert_eq!(counted(&c), [row("client_tls", "poll", "ok")]);
    }

    #[test]
    fn positive_sighup_is_counted_with_its_trigger() {
        let (tls, _) = tls_from_files(true);
        let (r, c) = reloader(Some(tls), None);
        r.reload(Trigger::Sighup);
        assert_eq!(counted(&c), [row("tls", "sighup", "ok")]);
    }

    #[test]
    fn corner_nothing_configured_never_reloads() {
        let (r, c) = reloader(None, None);
        let mut p = Poller::new(r);
        assert_eq!(ticks(&mut p, 3), [None, None, None]);
        assert!(counted(&c).is_empty());
    }

    #[test]
    fn corner_unconfigured_file_beside_the_cert_does_not_reload() {
        let (tls, cert) = tls_from_files(false);
        let (r, c) = reloader(Some(tls), None);
        let mut p = Poller::new(r);
        std::fs::write(cert.with_file_name("notes.txt"), "unrelated").unwrap();
        assert_eq!(ticks(&mut p, 1), [None]);
        assert!(counted(&c).is_empty());
    }

    #[test]
    fn corner_file_gone_mid_swap_then_back_unchanged_settles() {
        let (tls, cert) = tls_from_files(false);
        let (r, _) = reloader(Some(tls), None);
        let mut p = Poller::new(r);
        let pem = std::fs::read(&cert).unwrap();
        std::fs::remove_file(&cert).unwrap();
        assert_eq!(ticks(&mut p, 1), [Some(vec![false])]);
        // Back as it was when loaded: nothing to do.
        std::fs::write(&cert, pem).unwrap();
        assert_eq!(ticks(&mut p, 1), [None]);
    }

    #[test]
    fn corner_zero_never_spawns_the_poll() {
        let (tls, _) = tls_from_files(false);
        let (r, _) = reloader(Some(tls), None);
        // No runtime here: `0` must return before spawning anything.
        assert!(watch_poll(0, r).is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn boundary_one_second_poll_reloads_a_rotation_unsignalled() {
        let pki = TestPki::new("ca");
        let mut secret = SecretDir::new(&pki);
        let (r, c) = reloader(Some(secret.load()), None);
        let task = watch_poll(1, r).expect("a poll task");
        secret.rotate(&pki);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        while counted(&c).is_empty() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        task.abort();
        assert_eq!(counted(&c), [row("tls", "poll", "ok")]);
    }

    #[test]
    fn adversarial_cert_grown_past_the_cap_is_refused() {
        let (tls, cert) = tls_from_files(false);
        let (r, _) = reloader(Some(tls), None);
        let mut p = Poller::new(r);
        let mut huge = b"-----BEGIN CERTIFICATE-----\n".to_vec();
        huge.resize(agent_grpc::tls::MAX_PEM_BYTES as usize * 4, b'A');
        std::fs::write(&cert, huge).unwrap();
        let out = p.tick().expect("a change");
        let e = out[0].result.as_ref().unwrap_err();
        assert!(e.contains("exceeds"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn adversarial_symlink_redirected_outside_the_dir_reloads_only_the_named_path() {
        // Paths are operator config: the configured name is followed wherever it
        // points, as the loaders do, and only that name is watched.
        let pki = TestPki::new("ca");
        let (here, there) = (agent_testkit::tempdir(), agent_testkit::tempdir());
        let (c0, k0) = pki.issue(&LeafSpec::service("seam")).write_to(&here, "a");
        let (cert, key) = (here.join("tls.crt"), here.join("tls.key"));
        std::os::unix::fs::symlink(&c0, &cert).unwrap();
        std::os::unix::fs::symlink(&k0, &key).unwrap();
        let (r, c) = reloader(Some(ServerTls::load(&cert, &key, None).unwrap()), None);
        let mut p = Poller::new(r);
        assert_eq!(p.reloader.files(), [cert.clone(), key.clone()]);
        let (c1, k1) = pki.issue(&LeafSpec::service("seam")).write_to(&there, "b");
        for (link, target) in [(&cert, &c1), (&key, &k1)] {
            let tmp = link.with_extension("tmp");
            std::os::unix::fs::symlink(target, &tmp).unwrap();
            std::fs::rename(&tmp, link).unwrap();
        }
        assert_eq!(ticks(&mut p, 2), [Some(vec![true]), None]);
        // A change to the old target, no longer named, is not watched.
        std::fs::write(&c0, "rewritten").unwrap();
        assert_eq!(ticks(&mut p, 1), [None]);
        assert_eq!(counted(&c), [row("tls", "poll", "ok")]);
    }

    #[cfg(unix)]
    #[test]
    fn adversarial_files_flapping_every_tick_reload_once_per_tick() {
        let pki = TestPki::new("ca");
        let mut secret = SecretDir::new(&pki);
        let (r, c) = reloader(Some(secret.load()), None);
        let mut p = Poller::new(r);
        for _ in 0..5 {
            secret.rotate(&pki);
            assert_eq!(ticks(&mut p, 1), [Some(vec![true])]);
        }
        assert_eq!(counted(&c).len(), 5, "{:?}", counted(&c));
    }
}
