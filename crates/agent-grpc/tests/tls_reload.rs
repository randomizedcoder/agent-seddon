//! Reloading a TLS listener's certificates in place (security-hardening S20).
//!
//! A real tonic server on `127.0.0.1:0` serves health over a [`ServerTls`] loaded
//! from files. Each case rewrites the files, calls [`ServerTls::reload`], and dials
//! again. Which CA a client must trust (or which client certificate the server takes)
//! tells which config the handshake used. Clients pass their `ClientTls` explicitly,
//! so the cases never touch the process-wide client TLS.

use std::path::PathBuf;
use std::time::Duration;

use agent_grpc::server::{base_router_with_auth, AuthLayer};
use agent_grpc::{ClientTls, Endpoint, ServerTls};
use agent_testkit::pki::{LeafSpec, TestPki};
use rstest::rstest;
use tokio::io::AsyncWriteExt;
use tonic_health::pb::health_client::HealthClient;
use tonic_health::pb::HealthCheckRequest;

/// The listener's files, the two CAs that issue into them, and the running server.
struct Listener {
    old_ca: TestPki,
    new_ca: TestPki,
    cert: PathBuf,
    key: PathBuf,
    client_ca: PathBuf,
    tls: ServerTls,
    dial: Endpoint,
}

impl Listener {
    /// Serve a leaf from `old_ca`. With `mutual`, client certificates must chain to
    /// `old_ca` too (the client-CA file).
    async fn start(mutual: bool) -> Self {
        let old_ca = TestPki::new("old CA");
        let new_ca = TestPki::new("new CA");
        let dir = agent_testkit::tempdir();
        let (cert, key) = old_ca
            .issue(&LeafSpec::service("seam"))
            .write_to(&dir, "server");
        let client_ca = old_ca.write_ca(&dir, "client-ca");
        let tls = ServerTls::load(&cert, &key, mutual.then_some(client_ca.as_path()))
            .expect("server tls");
        let bound = Endpoint::parse("127.0.0.1:0").bind().await.expect("bind");
        let dial = bound.dial_endpoint().expect("dial").with_tls(true);
        let (router, health) = base_router_with_auth(0, None, AuthLayer::disabled(), None).await;
        let served = tls.clone();
        tokio::spawn(async move {
            let _health = health;
            let _ = bound
                .serve(router, Some(&served), std::future::pending())
                .await;
        });
        Self {
            old_ca,
            new_ca,
            cert,
            key,
            client_ca,
            tls,
            dial,
        }
    }

    /// Put a leaf from `new_ca` in the listener's files (not yet reloaded).
    fn renew_from_new_ca(&self) {
        let leaf = self.new_ca.issue(&LeafSpec::service("seam"));
        std::fs::write(&self.cert, &leaf.cert_pem).unwrap();
        std::fs::write(&self.key, &leaf.key_pem).unwrap();
    }

    /// A client trusting only `ca`, presenting a leaf from `identity` when given.
    fn client(ca: &TestPki, identity: Option<&TestPki>) -> ClientTls {
        let identity = identity.map(|pki| {
            let leaf = pki.issue(&LeafSpec::service("client"));
            (leaf.cert_pem, leaf.key_pem)
        });
        ClientTls::from_pem(Some(ca.ca_pem()), identity, None).expect("client tls")
    }

    /// A health check over a **new** connection.
    async fn check(&self, tls: &ClientTls) -> Result<(), String> {
        let channel = self
            .dial
            .connect_lazy_with(Some(tls))
            .map_err(|e| e.to_string())?;
        health(&mut HealthClient::new(channel)).await
    }
}

async fn health(client: &mut HealthClient<tonic::transport::Channel>) -> Result<(), String> {
    let call = client.check(HealthCheckRequest {
        service: String::new(),
    });
    match tokio::time::timeout(Duration::from_secs(10), call).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(status)) => Err(status.to_string()),
        Err(_) => Err("timed out".into()),
    }
}

#[tokio::test]
async fn positive_reload_serves_the_new_certificate_to_new_connections() {
    let l = Listener::start(false).await;
    let trusts_old = Listener::client(&l.old_ca, None);
    let trusts_new = Listener::client(&l.new_ca, None);
    l.check(&trusts_old).await.expect("old cert before reload");
    assert!(l.check(&trusts_new).await.is_err(), "new CA before reload");

    l.renew_from_new_ca();
    // Written but not reloaded: still the old certificate.
    l.check(&trusts_old).await.expect("old cert until reload");
    l.tls.reload().expect("reload");

    l.check(&trusts_new).await.expect("new cert after reload");
    assert!(l.check(&trusts_old).await.is_err(), "old CA after reload");
}

#[tokio::test]
async fn positive_existing_connection_survives_reload() {
    let l = Listener::start(false).await;
    let channel = l
        .dial
        .connect_lazy_with(Some(&Listener::client(&l.old_ca, None)))
        .unwrap();
    let mut client = HealthClient::new(channel);
    health(&mut client).await.expect("before reload");
    l.renew_from_new_ca();
    l.tls.reload().expect("reload");
    // The negotiated session is untouched: the same connection keeps serving.
    health(&mut client)
        .await
        .expect("same connection after reload");
}

#[rstest]
#[case::negative_cert_not_pem("garbage")]
#[case::negative_key_removed("no_key")]
#[case::negative_half_written_renewal("cert_only")]
#[case::adversarial_key_from_another_pair("mismatch")]
#[tokio::test]
async fn failed_reload_keeps_serving_the_old_certificate(#[case] damage: &str) {
    let l = Listener::start(false).await;
    let trusts_old = Listener::client(&l.old_ca, None);
    let other = l.new_ca.issue(&LeafSpec::service("seam"));
    match damage {
        "garbage" => std::fs::write(&l.cert, "not a certificate").unwrap(),
        "no_key" => std::fs::remove_file(&l.key).unwrap(),
        // A renewal that replaced the certificate but not yet the key.
        "cert_only" => std::fs::write(&l.cert, &other.cert_pem).unwrap(),
        "mismatch" => {
            let stranger = l.old_ca.issue(&LeafSpec::service("seam"));
            std::fs::write(&l.cert, &other.cert_pem).unwrap();
            std::fs::write(&l.key, &stranger.key_pem).unwrap();
        }
        other => unreachable!("{other}"),
    }
    let e = l.tls.reload().unwrap_err();
    assert!(!e.is_empty(), "{damage}");
    l.check(&trusts_old)
        .await
        .unwrap_or_else(|e| panic!("{damage}: old cert must still serve: {e}"));
}

#[tokio::test]
async fn corner_reload_twice_is_idempotent() {
    let l = Listener::start(false).await;
    let trusts_new = Listener::client(&l.new_ca, None);
    l.renew_from_new_ca();
    l.tls.reload().expect("first reload");
    l.tls.reload().expect("second reload");
    l.check(&trusts_new).await.expect("new cert");
}

#[tokio::test]
async fn adversarial_client_ca_rotated_out_is_refused_after_reload() {
    // mTLS: the client CA file moves from the old CA to the new one. A client whose
    // certificate chains only to the old CA is refused on its next connection.
    let l = Listener::start(true).await;
    let old_client = Listener::client(&l.old_ca, Some(&l.old_ca));
    let new_client = Listener::client(&l.old_ca, Some(&l.new_ca));
    l.check(&old_client).await.expect("old client before");
    assert!(l.check(&new_client).await.is_err(), "new client before");

    std::fs::write(&l.client_ca, l.new_ca.ca_pem()).unwrap();
    l.tls.reload().expect("reload");

    l.check(&new_client).await.expect("new client after");
    assert!(l.check(&old_client).await.is_err(), "old client after");
}

#[tokio::test]
async fn adversarial_silent_and_plaintext_peers_do_not_stall_the_listener() {
    let l = Listener::start(false).await;
    let Endpoint::Tcp { hostport, .. } = &l.dial else {
        panic!("tcp");
    };
    // Peers that open a socket and never handshake, and one that speaks plaintext
    // HTTP/2 at a TLS port. Each handshake runs on its own, so none of them delays
    // a real client.
    let mut silent = Vec::new();
    for _ in 0..16 {
        silent.push(tokio::net::TcpStream::connect(hostport).await.unwrap());
    }
    let mut plain = tokio::net::TcpStream::connect(hostport).await.unwrap();
    plain
        .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        l.check(&Listener::client(&l.old_ca, None)),
    )
    .await
    .expect("a real client is not delayed by stalled handshakes")
    .expect("served");
    drop(silent);
}

#[tokio::test]
async fn negative_in_memory_tls_has_nothing_to_reload() {
    let pki = TestPki::new("ca");
    let leaf = pki.issue(&LeafSpec::service("seam"));
    let tls = ServerTls::from_pem(&leaf.cert_pem, &leaf.key_pem, None::<&str>).unwrap();
    let e = tls.reload().unwrap_err();
    assert!(e.contains("nothing to reload"), "{e}");
}

#[tokio::test]
async fn negative_unix_socket_refuses_tls() {
    let pki = TestPki::new("ca");
    let leaf = pki.issue(&LeafSpec::service("seam"));
    let tls = ServerTls::from_pem(&leaf.cert_pem, &leaf.key_pem, None::<&str>).unwrap();
    let sock = agent_testkit::tempdir().join("s.sock");
    let bound = Endpoint::Uds(sock).bind().await.expect("bind");
    let (router, _health) = base_router_with_auth(0, None, AuthLayer::disabled(), None).await;
    let e = bound
        .serve(router, Some(&tls), std::future::pending())
        .await
        .unwrap_err();
    assert!(e.to_string().contains("does not serve TLS"), "{e}");
}
