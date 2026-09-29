//! SIGHUP reloads a serve mode's TLS material and token signing keys without a
//! restart (security-hardening S20).
//!
//! What reloads: the listener's `[grpc.tls]` certificate, key and client CA
//! ([`agent_grpc::ServerTls::reload`]) and the `[auth.token]` `signing_key` /
//! `previous_key` ([`agent_grpc::server::AuthLayer::reload_keys`]). Each part is
//! reloaded on its own, and a part that fails keeps what it had: a half-written
//! renewal logs a warning and the listener keeps serving the old certificate.
//!
//! Pair it with the renewer, for example
//! `step ca renew --daemon --exec "kill -HUP <pid>" server.crt server.key`.

use agent_grpc::server::AuthLayer;
use agent_grpc::ServerTls;

/// What one part of a reload did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Reloaded {
    /// `tls` or `signing_key`.
    pub what: &'static str,
    /// What is in use now, or why the old material was kept.
    pub result: Result<String, String>,
}

/// Reload every part this serve mode holds, each independently.
pub(crate) fn reload_all(tls: Option<&ServerTls>, auth: &AuthLayer) -> Vec<Reloaded> {
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

fn log(outcomes: &[Reloaded]) {
    if outcomes.is_empty() {
        tracing::info!("SIGHUP: nothing to reload (no `[grpc.tls]` cert, no `[auth.token]`)");
    }
    for o in outcomes {
        match &o.result {
            Ok(now) => tracing::info!(what = o.what, now = %now, "reloaded on SIGHUP"),
            Err(e) => tracing::warn!(
                what = o.what,
                error = %e,
                "reload on SIGHUP failed; still using the previous material"
            ),
        }
    }
}

/// Reload on every SIGHUP for the life of the process. Installing the handler also
/// means a SIGHUP no longer terminates a serve mode (its default action), with or
/// without anything to reload.
pub(crate) fn watch_sighup(tls: Option<ServerTls>, auth: AuthLayer) {
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
                log(&reload_all(tls.as_ref(), &auth));
            }
        });
    }
    #[cfg(not(unix))]
    let _ = (tls, auth);
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
        let got = reload_all(Some(&tls), &AuthLayer::disabled());
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
        let got = reload_all(Some(&tls), &AuthLayer::disabled());
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
        let got = reload_all(Some(&tls), &AuthLayer::disabled());
        let e = got[0].result.as_ref().unwrap_err();
        assert!(e.contains("exceeds"), "{e}");
    }

    #[test]
    fn corner_nothing_configured_reloads_nothing() {
        assert!(reload_all(None, &AuthLayer::disabled()).is_empty());
    }

    #[test]
    fn boundary_plain_tls_names_no_client_ca() {
        let (tls, _) = tls_from_files(false);
        let got = reload_all(Some(&tls), &AuthLayer::disabled());
        assert_eq!(got[0].result, Ok("certificate and key".into()));
    }
}
