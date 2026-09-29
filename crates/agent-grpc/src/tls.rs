//! TLS / mTLS for the TCP transport (security-hardening S4, closes gap-analysis P0-5).
//!
//! Two halves, both built from PEM files an operator (or `nix run .#pki-dev`) put on
//! disk:
//!
//! - [`ServerTls`] — a listener's certificate + key, plus an optional client CA. A
//!   client CA makes the listener **mutual**: a peer without a certificate chaining
//!   to it is refused during the handshake. Handed to
//!   [`crate::transport::Bound::serve`], and reloadable in place (S20).
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
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use arc_swap::ArcSwap;
use tokio_rustls::rustls::server::WebPkiClientVerifier;
use tokio_rustls::rustls::{RootCertStore, ServerConfig};
use tokio_rustls::TlsAcceptor;
use tonic::transport::{Certificate, ClientTlsConfig, Identity};

/// Upper bound on one PEM file (cert chain, key, or CA bundle). A real chain is a
/// few KiB; anything past this is a misconfiguration (or a hostile path such as
/// `/dev/zero`), refused before it is buffered.
pub const MAX_PEM_BYTES: u64 = 1 << 20;

/// A listener's TLS material, validated at load and reloadable in place.
///
/// The listener runs its own `tokio_rustls` acceptor
/// ([`crate::transport::Bound::serve`]), and each handshake takes the config current
/// at that moment. [`Self::reload`] re-reads the same files and swaps the config in,
/// so a renewed certificate (or a rotated client CA) is used by new connections
/// while existing ones keep the session they negotiated (security-hardening S20).
/// Clones share the swap: the copy the reload trigger holds and the listener's are
/// one config.
#[derive(Clone)]
pub struct ServerTls {
    config: Arc<ArcSwap<ServerConfig>>,
    mutual: bool,
    files: Option<Arc<ServerTlsFiles>>,
}

/// Where a [`ServerTls`] was loaded from, so a reload reads the same paths. Whether
/// the listener is mutual is fixed by these paths: a reload can rotate the client CA
/// but never turn client certificates off.
#[derive(Debug)]
struct ServerTlsFiles {
    cert: PathBuf,
    key: PathBuf,
    client_ca: Option<PathBuf>,
}

impl ServerTlsFiles {
    fn build(&self) -> Result<ServerConfig, String> {
        let cert = read_pem(&self.cert)?;
        let key = read_pem(&self.key)?;
        warn_if_key_is_readable(&self.key);
        let ca = self.client_ca.as_deref().map(read_pem).transpose()?;
        server_config(&cert, &key, ca.as_deref())
    }
}

impl std::fmt::Debug for ServerTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerTls")
            .field("mutual", &self.mutual)
            .field("files", &self.files)
            .finish_non_exhaustive()
    }
}

impl ServerTls {
    /// Load `cert` (leaf first, then any intermediates) + `key`, and — when given —
    /// `client_ca`, which makes client certificates **required**.
    pub fn load(cert: &Path, key: &Path, client_ca: Option<&Path>) -> Result<Self, String> {
        let files = ServerTlsFiles {
            cert: cert.to_path_buf(),
            key: key.to_path_buf(),
            client_ca: client_ca.map(Path::to_path_buf),
        };
        let config = files.build()?;
        Ok(Self {
            config: Arc::new(ArcSwap::from_pointee(config)),
            mutual: client_ca.is_some(),
            files: Some(Arc::new(files)),
        })
    }

    /// Build from in-memory PEM (tests). Such a config has no files, so
    /// [`Self::reload`] refuses it.
    pub fn from_pem(
        cert: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
        client_ca: Option<impl AsRef<[u8]>>,
    ) -> Result<Self, String> {
        let mutual = client_ca.is_some();
        let config = server_config(
            cert.as_ref(),
            key.as_ref(),
            client_ca.as_ref().map(AsRef::as_ref),
        )?;
        Ok(Self {
            config: Arc::new(ArcSwap::from_pointee(config)),
            mutual,
            files: None,
        })
    }

    /// Re-read the files this was loaded from and, if they build a valid config,
    /// use it for every new handshake. On any error the current config stays in
    /// use, so a half-written renewal never takes the listener down.
    pub fn reload(&self) -> Result<(), String> {
        let files = self
            .files
            .as_ref()
            .ok_or("this TLS listener was not loaded from files; nothing to reload")?;
        let config = files.build()?;
        self.config.store(Arc::new(config));
        Ok(())
    }

    /// Whether client certificates are required (a client CA was configured).
    pub fn is_mutual(&self) -> bool {
        self.mutual
    }

    /// An acceptor over the config current now.
    pub(crate) fn acceptor(&self) -> TlsAcceptor {
        TlsAcceptor::from(self.config.load_full())
    }
}

/// Build a rustls server config the way tonic's own acceptor does (the certificate
/// chain + key, a WebPKI client verifier when a client CA is given, ALPN `h2`), so
/// running our own acceptor changes nothing on the wire. rustls checks here that the
/// key belongs to the leaf certificate, so a mismatched pair fails at load (or at
/// reload), not per connection.
fn server_config(
    cert: &[u8],
    key: &[u8],
    client_ca: Option<&[u8]>,
) -> Result<ServerConfig, String> {
    let invalid =
        |what: &str, e: &dyn std::fmt::Display| format!("invalid server TLS material: {what}: {e}");
    let chain = rustls_pemfile::certs(&mut &*cert)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| invalid("certificate", &e))?;
    if chain.is_empty() {
        return Err("invalid server TLS material: no certificate in the cert file".into());
    }
    let key = rustls_pemfile::private_key(&mut &*key)
        .map_err(|e| invalid("private key", &e))?
        .ok_or("invalid server TLS material: no private key in the key file")?;
    let builder = ServerConfig::builder();
    let builder = match client_ca {
        None => builder.with_no_client_auth(),
        Some(ca) => {
            let mut roots = RootCertStore::empty();
            for c in rustls_pemfile::certs(&mut &*ca) {
                let c = c.map_err(|e| invalid("client CA", &e))?;
                roots.add(c).map_err(|e| invalid("client CA", &e))?;
            }
            if roots.is_empty() {
                return Err("invalid server TLS material: no certificate in the client CA".into());
            }
            let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
                .build()
                .map_err(|e| invalid("client CA", &e))?;
            builder.with_client_cert_verifier(verifier)
        }
    };
    let mut config = builder
        .with_single_cert(chain, key)
        .map_err(|e| invalid("certificate and key", &e))?;
    config.alpn_protocols = vec![b"h2".to_vec()];
    Ok(config)
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
pub(crate) fn read_pem(path: &Path) -> Result<Vec<u8>, String> {
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
pub(crate) fn warn_if_key_is_readable(key: &Path) {
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
