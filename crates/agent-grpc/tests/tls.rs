//! The TLS / mTLS transport matrix (security-hardening S4): a real tonic server on
//! `127.0.0.1:0` with certificates from a throwaway in-memory CA
//! (`agent_testkit::pki`), probed with `grpc.health.v1.Health/Check`.
//!
//! Every case passes its `ClientTls` explicitly (`connect_lazy_with`), so the matrix
//! never touches the process-wide client TLS and the cases run in parallel safely.

use std::time::Duration;

use agent_grpc::server::{base_router_with_auth, AuthLayer};
use agent_grpc::{ClientTls, Endpoint, ServerTls};
use agent_testkit::pki::{Issued, LeafSpec, TestPki, Validity};
use rstest::rstest;
use tonic_health::pb::health_client::HealthClient;
use tonic_health::pb::HealthCheckRequest;
use tonic_health::ServingStatus;

/// The certificate the server presents.
#[derive(Clone, Copy, Debug)]
enum ServerCert {
    /// No TLS at all (a plaintext listener).
    Plaintext,
    /// A current service leaf from the trusted CA (SANs cover `127.0.0.1`).
    Good,
    Expired,
    NotYetValid,
    /// A current leaf from an unrelated CA the client does not trust.
    OtherCa,
    /// A current leaf whose only SAN is `seam.internal` (not the dialed IP).
    DnsOnly,
}

/// What the client trusts and presents.
#[derive(Clone, Copy, Debug)]
enum ClientSide {
    /// Trust the test CA; no client certificate.
    Ca,
    /// Trust the test CA and present a leaf it issued.
    CaAndCert,
    /// Trust the test CA but present a leaf from an unrelated CA.
    CaAndForeignCert,
    /// Trust the test CA and verify the server as `seam.internal`.
    CaWithDomain,
    /// No CA configured: the public web roots only.
    WebRoots,
}

/// How the address is written.
#[derive(Clone, Copy, Debug)]
enum Dial {
    Https,
    /// Bare `host:port` — plaintext even when client TLS is configured.
    Plain,
}

struct Pkis {
    ca: TestPki,
    other: TestPki,
}

impl Pkis {
    fn new() -> Self {
        Self {
            ca: TestPki::new("agent test CA"),
            other: TestPki::new("unrelated CA"),
        }
    }

    fn server(&self, cert: ServerCert, mutual: bool) -> Option<ServerTls> {
        let leaf: Issued = match cert {
            ServerCert::Plaintext => return None,
            ServerCert::Good => self.ca.issue(&LeafSpec::service("seam")),
            ServerCert::Expired => self
                .ca
                .issue(&LeafSpec::service("seam").with_validity(Validity::Expired)),
            ServerCert::NotYetValid => self
                .ca
                .issue(&LeafSpec::service("seam").with_validity(Validity::NotYetValid)),
            ServerCert::OtherCa => self.other.issue(&LeafSpec::service("seam")),
            ServerCert::DnsOnly => self.ca.issue(&LeafSpec::dns_only("seam.internal")),
        };
        let client_ca = mutual.then(|| self.ca.ca_pem());
        Some(ServerTls::from_pem(&leaf.cert_pem, &leaf.key_pem, client_ca).expect("server tls"))
    }

    fn client(&self, side: ClientSide) -> ClientTls {
        let ca = Some(self.ca.ca_pem());
        let own = self.ca.issue(&LeafSpec::service("client"));
        let foreign = self.other.issue(&LeafSpec::service("client"));
        let result = match side {
            ClientSide::Ca => ClientTls::from_pem(ca, None::<(&str, &str)>, None),
            ClientSide::CaAndCert => {
                ClientTls::from_pem(ca, Some((own.cert_pem, own.key_pem)), None)
            }
            ClientSide::CaAndForeignCert => {
                ClientTls::from_pem(ca, Some((foreign.cert_pem, foreign.key_pem)), None)
            }
            ClientSide::CaWithDomain => {
                ClientTls::from_pem(ca, None::<(&str, &str)>, Some("seam.internal"))
            }
            ClientSide::WebRoots => ClientTls::from_pem(None::<&str>, None::<(&str, &str)>, None),
        };
        result.expect("client tls")
    }
}

/// Serve health on an ephemeral loopback port; returns the plaintext dial endpoint.
async fn serve(endpoint: Endpoint, tls: Option<ServerTls>) -> Endpoint {
    let bound = endpoint.bind().await.expect("bind");
    let dial = bound.dial_endpoint().expect("dial endpoint");
    let (router, health) = base_router_with_auth(0, None, AuthLayer::disabled(), None).await;
    let served = tls.clone();
    tokio::spawn(async move {
        let _health = health; // keep the reporter alive for the server's lifetime
        let _ = bound
            .serve(router, served.as_ref(), std::future::pending())
            .await;
    });
    dial
}

async fn check(endpoint: &Endpoint, tls: Option<&ClientTls>) -> Result<(), String> {
    let channel = endpoint.connect_lazy_with(tls).map_err(|e| e.to_string())?;
    let mut client = HealthClient::new(channel);
    let call = client.check(HealthCheckRequest {
        service: String::new(),
    });
    match tokio::time::timeout(Duration::from_secs(10), call).await {
        Ok(Ok(resp)) if resp.get_ref().status == ServingStatus::Serving as i32 => Ok(()),
        Ok(Ok(resp)) => Err(format!("unexpected status {}", resp.get_ref().status)),
        Ok(Err(status)) => Err(status.to_string()),
        Err(_) => Err("timed out".into()),
    }
}

#[rstest]
#[case::positive_https_dials_tls(ServerCert::Good, false, ClientSide::Ca, Dial::Https, true)]
#[case::positive_mtls_roundtrip(ServerCert::Good, true, ClientSide::CaAndCert, Dial::Https, true)]
#[case::positive_domain_override(
    ServerCert::DnsOnly,
    false,
    ClientSide::CaWithDomain,
    Dial::Https,
    true
)]
#[case::negative_expired_server_cert(
    ServerCert::Expired,
    false,
    ClientSide::Ca,
    Dial::Https,
    false
)]
#[case::negative_not_yet_valid_server_cert(
    ServerCert::NotYetValid,
    false,
    ClientSide::Ca,
    Dial::Https,
    false
)]
#[case::negative_plaintext_client_to_tls_server(
    ServerCert::Good,
    false,
    ClientSide::Ca,
    Dial::Plain,
    false
)]
#[case::negative_mtls_without_client_cert(
    ServerCert::Good,
    true,
    ClientSide::Ca,
    Dial::Https,
    false
)]
#[case::negative_web_roots_do_not_trust_private_ca(
    ServerCert::Good,
    false,
    ClientSide::WebRoots,
    Dial::Https,
    false
)]
#[case::negative_tls_client_to_plaintext_server(
    ServerCert::Plaintext,
    false,
    ClientSide::Ca,
    Dial::Https,
    false
)]
#[case::adversarial_client_cert_from_other_ca(
    ServerCert::Good,
    true,
    ClientSide::CaAndForeignCert,
    Dial::Https,
    false
)]
#[case::adversarial_server_cert_from_other_ca(
    ServerCert::OtherCa,
    false,
    ClientSide::Ca,
    Dial::Https,
    false
)]
#[case::adversarial_server_name_mismatch(
    ServerCert::DnsOnly,
    false,
    ClientSide::Ca,
    Dial::Https,
    false
)]
#[case::corner_bare_hostport_stays_plaintext(
    ServerCert::Plaintext,
    false,
    ClientSide::CaAndCert,
    Dial::Plain,
    true
)]
#[case::boundary_client_cert_to_non_mutual_server_ignored(
    ServerCert::Good,
    false,
    ClientSide::CaAndCert,
    Dial::Https,
    true
)]
#[tokio::test]
async fn tls_matrix(
    #[case] server_cert: ServerCert,
    #[case] mutual: bool,
    #[case] client_side: ClientSide,
    #[case] dial: Dial,
    #[case] ok: bool,
) {
    let pkis = Pkis::new();
    let server_tls = pkis.server(server_cert, mutual);
    let endpoint = serve(Endpoint::tcp("127.0.0.1:0"), server_tls).await;
    let endpoint = endpoint.with_tls(matches!(dial, Dial::Https));
    let client = pkis.client(client_side);
    let got = check(&endpoint, Some(&client)).await;
    assert_eq!(
        got.is_ok(),
        ok,
        "{server_cert:?}/{mutual}/{client_side:?}/{dial:?}: {got:?}"
    );
}

/// A unix socket never uses TLS: a client with full mTLS config still dials it
/// plaintext, so configuring client TLS cannot break a local UDS seam.
#[tokio::test]
async fn corner_uds_unaffected_by_client_tls() {
    let pkis = Pkis::new();
    let path = agent_testkit::tempdir().join("tls-matrix.sock");
    let endpoint = serve(Endpoint::Uds(path), None).await;
    let client = pkis.client(ClientSide::CaAndCert);
    check(&endpoint.with_tls(true), Some(&client))
        .await
        .expect("uds stays plaintext");
}

/// With no explicit client TLS an `https://` dial still verifies (public web roots)
/// rather than silently going plaintext, so a private-CA server is refused.
#[tokio::test]
async fn negative_https_without_client_tls_verifies_against_web_roots() {
    let pkis = Pkis::new();
    let endpoint = serve(
        Endpoint::tcp("127.0.0.1:0"),
        pkis.server(ServerCert::Good, false),
    )
    .await;
    assert!(check(&endpoint.with_tls(true), None).await.is_err());
}
