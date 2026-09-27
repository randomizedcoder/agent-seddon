//! TLS / mTLS for the TCP transport (security-hardening S4, closes gap-analysis P0-5).
//!
//! Two halves, both built from PEM files an operator (or `nix run .#pki-dev`) put on
//! disk:
//!
//! - [`ServerTls`] — a listener's certificate + key, plus an optional client CA. A
//!   client CA makes the listener **mutual**: a peer without a certificate chaining
//!   to it is refused during the handshake. Handed to
//!   [`crate::server::base_router_with_tls`].
//! - [`ClientTls`] — what an `https://` dial trusts (the configured CA **only**, or
//!   the public web roots when none is set), an optional client identity for mTLS,
//!   and an optional server-name override. Installed process-wide with
//!   [`set_client_tls`] so every `= "grpc"` seam client's
//!   [`crate::Endpoint::connect_lazy`] picks it up without threading it through
//!   ~50 constructors.
//!
//! Which dials use TLS is decided by the **address**, not the config: `https://host:port`
//! is TLS, `http://host:port` and bare `host:port` stay plaintext (back-compat), and a
//! `unix:` socket never uses TLS. The config only says *how* to do TLS.
//!
//! The files are operator config, not model input, but they are still read
//! defensively: size-capped before buffering ([`MAX_PEM_BYTES`]), required to look
//! like PEM, and validated up front by building the rustls config — a bad file
//! fails at startup, not on the first handshake.

use std::io::Read;
use std::path::Path;
use std::sync::{Arc, RwLock};

use tonic::transport::{Certificate, ClientTlsConfig, Identity, ServerTlsConfig};

/// Upper bound on one PEM file (cert chain, key, or CA bundle). A real chain is a
/// few KiB; anything past this is a misconfiguration (or a hostile path such as
/// `/dev/zero`), refused before it is buffered.
pub const MAX_PEM_BYTES: u64 = 1 << 20;

/// A listener's TLS material, validated at load.
#[derive(Clone, Debug)]
pub struct ServerTls {
    config: ServerTlsConfig,
    mutual: bool,
}

impl ServerTls {
    /// Load `cert` (leaf first, then any intermediates) + `key`, and — when given —
    /// `client_ca`, which makes client certificates **required**.
    pub fn load(cert: &Path, key: &Path, client_ca: Option<&Path>) -> Result<Self, String> {
        let cert_pem = read_pem(cert)?;
        let key_pem = read_pem(key)?;
        warn_if_key_is_readable(key);
        let ca_pem = client_ca.map(read_pem).transpose()?;
        Self::from_pem(cert_pem, key_pem, ca_pem)
    }

    /// Build from in-memory PEM (tests, and [`Self::load`] after reading).
    pub fn from_pem(
        cert: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
        client_ca: Option<impl AsRef<[u8]>>,
    ) -> Result<Self, String> {
        let mut config = ServerTlsConfig::new().identity(Identity::from_pem(cert, key));
        let mutual = client_ca.is_some();
        if let Some(ca) = client_ca {
            config = config.client_ca_root(Certificate::from_pem(ca));
        }
        // Build the acceptor once now purely to validate: a key that does not parse
        // or match fails here with the file names in hand, not per connection.
        tonic::transport::Server::builder()
            .tls_config(config.clone())
            .map_err(|e| format!("invalid server TLS material: {}", error_chain(&e)))?;
        Ok(Self { config, mutual })
    }

    /// Whether client certificates are required (a client CA was configured).
    pub fn is_mutual(&self) -> bool {
        self.mutual
    }

    /// The tonic server config.
    pub fn config(&self) -> ServerTlsConfig {
        self.config.clone()
    }
}

/// How an `https://` dial authenticates the server (and, for mTLS, itself).
#[derive(Clone, Debug, Default)]
pub struct ClientTls {
    ca: Option<Certificate>,
    identity: Option<Identity>,
    domain: Option<String>,
}

impl ClientTls {
    /// Load the optional pieces: `ca` (trust **only** this CA; absent ⇒ public web
    /// roots), `cert` + `key` (present both or neither ⇒ client identity for mTLS),
    /// and `domain` (the name the server certificate must carry, overriding the
    /// dialed host).
    pub fn load(
        ca: Option<&Path>,
        cert: Option<&Path>,
        key: Option<&Path>,
        domain: Option<&str>,
    ) -> Result<Self, String> {
        let ca = ca.map(read_pem).transpose()?;
        let identity = match (cert, key) {
            (Some(cert), Some(key)) => {
                let pair = (read_pem(cert)?, read_pem(key)?);
                warn_if_key_is_readable(key);
                Some(pair)
            }
            (None, None) => None,
            _ => return Err("client TLS cert and key must be set together".into()),
        };
        Self::from_pem(ca, identity, domain)
    }

    /// Build from in-memory PEM; validated by building a connector.
    pub fn from_pem(
        ca: Option<impl AsRef<[u8]>>,
        identity: Option<(impl AsRef<[u8]>, impl AsRef<[u8]>)>,
        domain: Option<&str>,
    ) -> Result<Self, String> {
        if let Some(domain) = domain {
            validate_domain(domain)?;
        }
        let tls = Self {
            ca: ca.map(Certificate::from_pem),
            identity: identity.map(|(cert, key)| Identity::from_pem(cert, key)),
            domain: domain.map(str::to_owned),
        };
        tonic::transport::Endpoint::from_static("https://localhost")
            .tls_config(tls.config_for("localhost"))
            .map_err(|e| format!("invalid client TLS material: {}", error_chain(&e)))?;
        Ok(tls)
    }

    /// Whether this client presents a certificate (mTLS).
    pub fn has_identity(&self) -> bool {
        self.identity.is_some()
    }

    /// The tonic client config for a dial to `host`: the configured server name
    /// wins, else the dialed host (IPv6 brackets stripped, so `[::1]` verifies
    /// against an `::1` IP SAN).
    pub fn config_for(&self, host: &str) -> ClientTlsConfig {
        let domain = self.domain.clone().unwrap_or_else(|| {
            host.trim_start_matches('[')
                .trim_end_matches(']')
                .to_owned()
        });
        let mut config = ClientTlsConfig::new().domain_name(domain);
        config = match &self.ca {
            // A configured CA is the **only** trust anchor: mixing in the web roots
            // would let any public CA mint a certificate for an internal seam name.
            Some(ca) => config.ca_certificate(ca.clone()),
            None => config.with_webpki_roots(),
        };
        if let Some(identity) = &self.identity {
            config = config.identity(identity.clone());
        }
        config
    }
}

/// The process-wide client TLS used by [`crate::Endpoint::connect_lazy`] for
/// `https://` dials. `RwLock` (not `OnceLock`, cf. the authz observer) because a
/// config reload — and tests — replace it.
static CLIENT_TLS: RwLock<Option<Arc<ClientTls>>> = RwLock::new(None);

/// Install (or, with `None`, clear) the process-wide client TLS. Replace semantics.
pub fn set_client_tls(tls: Option<ClientTls>) {
    *CLIENT_TLS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = tls.map(Arc::new);
}

/// The process-wide client TLS, if one was installed.
pub fn client_tls() -> Option<Arc<ClientTls>> {
    CLIENT_TLS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// A server-name override is written into every handshake's SNI and checked
/// against the certificate, so keep it to hostname/IP characters: no whitespace,
/// scheme, path, port or wildcard.
fn validate_domain(domain: &str) -> Result<(), String> {
    let ok_host = !domain.is_empty()
        && domain.len() <= 253
        && !domain.starts_with(['.', '-'])
        && domain
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    if ok_host || domain.parse::<std::net::IpAddr>().is_ok() {
        Ok(())
    } else {
        Err("client TLS domain must be a hostname ([A-Za-z0-9.-]) or an IP address".into())
    }
}

/// Read one PEM file: size-capped before buffering, and required to contain a PEM
/// block (a DER file or a wrong path fails with a clear message rather than a
/// rustls parse error).
fn read_pem(path: &Path) -> Result<Vec<u8>, String> {
    let shown = path.display();
    let file = std::fs::File::open(path).map_err(|e| format!("TLS file `{shown}`: {e}"))?;
    let mut buf = Vec::new();
    file.take(MAX_PEM_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("TLS file `{shown}`: {e}"))?;
    if buf.len() as u64 > MAX_PEM_BYTES {
        return Err(format!(
            "TLS file `{shown}` exceeds {MAX_PEM_BYTES} bytes; refusing to load it"
        ));
    }
    if !buf.windows(10).any(|w| w == b"-----BEGIN") {
        return Err(format!(
            "TLS file `{shown}` is not PEM (no `-----BEGIN` block)"
        ));
    }
    Ok(buf)
}

/// A private key readable by group/other is worth a warning (not a refusal: a
/// deployment may deliberately share it with a group-owned sidecar).
fn warn_if_key_is_readable(key: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(key) {
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            tracing::warn!(
                key = %key.display(),
                mode = format!("{mode:o}"),
                "a gRPC TLS private key is group/world-accessible; consider `chmod 600`"
            );
        }
    }
}

/// `tonic::transport::Error`'s own `Display` is just "transport error"; the cause
/// is in the source chain.
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        out.push_str(": ");
        out.push_str(&s.to_string());
        source = s.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::pki::{LeafSpec, TestPki};
    use rstest::rstest;

    #[rstest]
    #[case::positive_hostname("seam.internal", true)]
    #[case::positive_ipv4("127.0.0.1", true)]
    #[case::positive_ipv6("::1", true)]
    #[case::boundary_max_len(&"a".repeat(253), true)]
    #[case::boundary_over_max_len(&"a".repeat(254), false)]
    #[case::negative_empty("", false)]
    #[case::corner_leading_dot(".internal", false)]
    #[case::adversarial_wildcard("*.internal", false)]
    #[case::adversarial_scheme("https://seam", false)]
    #[case::adversarial_port("seam:443", false)]
    #[case::adversarial_whitespace("seam internal", false)]
    #[case::adversarial_nul("seam\0evil", false)]
    fn validate_domain_cases(#[case] domain: &str, #[case] ok: bool) {
        assert_eq!(validate_domain(domain).is_ok(), ok, "{domain:?}");
    }

    #[rstest]
    #[case::positive_pem(b"-----BEGIN CERTIFICATE-----\nAA==\n-----END CERTIFICATE-----\n".to_vec(), None)]
    #[case::negative_not_pem(b"\x30\x82\x01\x0a not pem".to_vec(), Some("not PEM"))]
    #[case::corner_empty(Vec::new(), Some("not PEM"))]
    #[case::boundary_at_cap({
        let mut v = b"-----BEGIN X-----\n".to_vec();
        v.resize(MAX_PEM_BYTES as usize, b'A');
        v
    }, None)]
    #[case::adversarial_oversized({
        let mut v = b"-----BEGIN X-----\n".to_vec();
        v.resize(MAX_PEM_BYTES as usize + 1, b'A');
        v
    }, Some("exceeds"))]
    fn read_pem_cases(#[case] content: Vec<u8>, #[case] err: Option<&str>) {
        let path = agent_testkit::tempdir().join("f.pem");
        std::fs::write(&path, content).unwrap();
        match (read_pem(&path), err) {
            (Ok(_), None) => {}
            (Err(e), Some(want)) => assert!(e.contains(want), "{e}"),
            (got, want) => panic!("got {got:?}, want err {want:?}"),
        }
    }

    #[test]
    fn negative_read_pem_missing_file_names_the_path() {
        let e = read_pem(Path::new("/nonexistent/agent-seddon/x.pem")).unwrap_err();
        assert!(e.contains("/nonexistent/agent-seddon/x.pem"), "{e}");
    }

    #[test]
    fn positive_server_tls_loads_and_reports_mutual() {
        let pki = TestPki::new("test ca");
        let leaf = pki.issue(&LeafSpec::service("seam"));
        let plain = ServerTls::from_pem(&leaf.cert_pem, &leaf.key_pem, None::<&str>).unwrap();
        assert!(!plain.is_mutual());
        let mutual =
            ServerTls::from_pem(&leaf.cert_pem, &leaf.key_pem, Some(pki.ca_pem())).unwrap();
        assert!(mutual.is_mutual());
    }

    #[test]
    fn adversarial_server_tls_key_from_another_pair_rejected() {
        let pki = TestPki::new("test ca");
        let a = pki.issue(&LeafSpec::service("a"));
        let b = pki.issue(&LeafSpec::service("b"));
        let e = ServerTls::from_pem(&a.cert_pem, &b.key_pem, None::<&str>).unwrap_err();
        assert!(e.contains("invalid server TLS material"), "{e}");
    }

    #[test]
    fn negative_server_tls_garbage_key_rejected() {
        let pki = TestPki::new("test ca");
        let a = pki.issue(&LeafSpec::service("a"));
        let bad = "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n";
        assert!(ServerTls::from_pem(&a.cert_pem, bad, None::<&str>).is_err());
    }

    #[rstest]
    #[case::positive_ca_only(true, false, None, true)]
    #[case::positive_webpki_default(false, false, None, true)]
    #[case::positive_mtls_identity(true, true, Some("seam.internal"), true)]
    #[case::adversarial_bad_domain(true, false, Some("seam/../x"), false)]
    fn client_tls_from_pem_cases(
        #[case] with_ca: bool,
        #[case] with_identity: bool,
        #[case] domain: Option<&str>,
        #[case] ok: bool,
    ) {
        let pki = TestPki::new("test ca");
        let leaf = pki.issue(&LeafSpec::service("client"));
        let ca = with_ca.then(|| pki.ca_pem());
        let identity = with_identity.then(|| (leaf.cert_pem.clone(), leaf.key_pem.clone()));
        let got = ClientTls::from_pem(ca, identity, domain);
        assert_eq!(got.is_ok(), ok, "{got:?}");
        if let Ok(tls) = got {
            assert_eq!(tls.has_identity(), with_identity);
        }
    }

    #[test]
    fn negative_client_tls_cert_without_key_rejected() {
        let pki = TestPki::new("test ca");
        let (cert, _key) = pki
            .issue(&LeafSpec::service("c"))
            .write_to(&agent_testkit::tempdir(), "c");
        let e = ClientTls::load(None, Some(&cert), None, None).unwrap_err();
        assert!(e.contains("set together"), "{e}");
    }

    #[test]
    fn corner_set_client_tls_replaces_and_clears() {
        // The only test touching the process-global; the wire tests pass their
        // `ClientTls` explicitly via `connect_lazy_with` so they cannot race it.
        set_client_tls(Some(ClientTls::default()));
        assert!(client_tls().is_some());
        set_client_tls(None);
        assert!(client_tls().is_none());
    }
}
