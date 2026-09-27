//! Throwaway X.509 PKI for the gRPC TLS / mTLS matrix (security-hardening S4).
//!
//! [`TestPki::new`] mints a self-signed ECDSA P-256 CA; [`TestPki::issue`] signs leaf
//! certificates under it. Everything is generated in memory at test time, so no key
//! material is committed and every test can hold its own CA (the "certificate from
//! another CA" cases need a second, unrelated one).
//!
//! Leaves carry both `serverAuth` and `clientAuth`, the SAN shape the dev PKI
//! (`nix run .#pki-dev`) uses: `localhost`, `127.0.0.1`, `::1`, the service name, and
//! `spiffe://agent.test/svc/<name>`.
//!
//! ```ignore
//! let pki = TestPki::new("test CA");
//! let server = pki.issue(&LeafSpec::service("svc-a"));
//! let (cert, key) = server.write_to(&dir, "svc-a");
//! ```

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};

use rcgen::{
    date_time_ymd, BasicConstraints, Certificate, CertificateParams, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SanType,
};

/// The trust domain every test leaf's SPIFFE id lives under.
pub const TRUST_DOMAIN: &str = "agent.test";

/// When a leaf is valid, relative to the wall clock of the test run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validity {
    /// 2020-01-01 .. 2099-01-01.
    Current,
    /// 2000-01-01 .. 2001-01-01 (long expired).
    Expired,
    /// 2098-01-01 .. 2099-01-01 (not valid yet).
    NotYetValid,
}

/// What to put in a leaf certificate.
#[derive(Clone, Debug)]
pub struct LeafSpec {
    /// The subject common name.
    pub name: String,
    /// DNS subject alternative names.
    pub dns: Vec<String>,
    /// IP subject alternative names.
    pub ips: Vec<IpAddr>,
    /// A URI SAN (the SPIFFE id), if any.
    pub uri: Option<String>,
    pub validity: Validity,
}

impl LeafSpec {
    /// A service leaf: `localhost`, `127.0.0.1`, `::1`, `name`, and
    /// `spiffe://agent.test/svc/<name>`, currently valid.
    pub fn service(name: &str) -> Self {
        Self {
            name: name.to_string(),
            dns: vec!["localhost".into(), name.to_string()],
            ips: vec![
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V6(Ipv6Addr::LOCALHOST),
            ],
            uri: Some(format!("spiffe://{TRUST_DOMAIN}/svc/{name}")),
            validity: Validity::Current,
        }
    }

    /// Only the given DNS name: no loopback names or addresses.
    pub fn dns_only(name: &str) -> Self {
        Self {
            name: name.to_string(),
            dns: vec![name.to_string()],
            ips: Vec::new(),
            uri: None,
            validity: Validity::Current,
        }
    }

    pub fn with_validity(mut self, validity: Validity) -> Self {
        self.validity = validity;
        self
    }
}

/// A certificate and its private key, both PEM.
#[derive(Clone, Debug)]
pub struct Issued {
    pub cert_pem: String,
    pub key_pem: String,
}

impl Issued {
    /// Write `<stem>.crt` and `<stem>.key` into `dir`; returns their paths.
    pub fn write_to(&self, dir: &Path, stem: &str) -> (PathBuf, PathBuf) {
        let cert = dir.join(format!("{stem}.crt"));
        let key = dir.join(format!("{stem}.key"));
        std::fs::write(&cert, &self.cert_pem).expect("write cert");
        std::fs::write(&key, &self.key_pem).expect("write key");
        (cert, key)
    }
}

/// A self-signed CA that issues leaves.
pub struct TestPki {
    cert: Certificate,
    key: KeyPair,
}

impl TestPki {
    /// A fresh CA named `common_name`.
    pub fn new(common_name: &str) -> Self {
        let key = KeyPair::generate().expect("generate CA key");
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("CA params");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(DnType::CommonName, common_name);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        set_window(&mut params, Validity::Current);
        let cert = params.self_signed(&key).expect("self-sign CA");
        Self { cert, key }
    }

    /// The CA certificate, PEM.
    pub fn ca_pem(&self) -> String {
        self.cert.pem()
    }

    /// Write the CA certificate to `dir/<stem>.crt`; returns its path.
    pub fn write_ca(&self, dir: &Path, stem: &str) -> PathBuf {
        let path = dir.join(format!("{stem}.crt"));
        std::fs::write(&path, self.ca_pem()).expect("write CA cert");
        path
    }

    /// Sign a leaf described by `spec`.
    pub fn issue(&self, spec: &LeafSpec) -> Issued {
        let key = KeyPair::generate().expect("generate leaf key");
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("leaf params");
        params
            .distinguished_name
            .push(DnType::CommonName, &spec.name);
        for dns in &spec.dns {
            params
                .subject_alt_names
                .push(SanType::DnsName(dns.clone().try_into().expect("DNS SAN")));
        }
        for ip in &spec.ips {
            params.subject_alt_names.push(SanType::IpAddress(*ip));
        }
        if let Some(uri) = &spec.uri {
            params
                .subject_alt_names
                .push(SanType::URI(uri.clone().try_into().expect("URI SAN")));
        }
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        set_window(&mut params, spec.validity);
        let cert = params
            .signed_by(&key, &self.cert, &self.key)
            .expect("sign leaf");
        Issued {
            cert_pem: cert.pem(),
            key_pem: key.serialize_pem(),
        }
    }
}

fn set_window(params: &mut CertificateParams, validity: Validity) {
    let (from, to) = match validity {
        Validity::Current => ((2020, 1, 1), (2099, 1, 1)),
        Validity::Expired => ((2000, 1, 1), (2001, 1, 1)),
        Validity::NotYetValid => ((2098, 1, 1), (2099, 1, 1)),
    };
    params.not_before = date_time_ymd(from.0, from.1, from.2);
    params.not_after = date_time_ymd(to.0, to.1, to.2);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::positive_service_leaf(LeafSpec::service("svc-a"), "svc-a")]
    #[case::corner_dns_only_leaf(LeafSpec::dns_only("svc.agent.internal"), "svc.agent.internal")]
    #[case::boundary_expired_leaf_still_issues(
        LeafSpec::service("old").with_validity(Validity::Expired),
        "old"
    )]
    fn issue_cases(#[case] spec: LeafSpec, #[case] _name: &str) {
        let pki = TestPki::new("testkit CA");
        let issued = pki.issue(&spec);
        assert!(issued.cert_pem.starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(issued.key_pem.contains("PRIVATE KEY-----"));
        assert!(pki.ca_pem().starts_with("-----BEGIN CERTIFICATE-----"));
    }

    #[test]
    fn negative_two_cas_differ() {
        assert_ne!(TestPki::new("a").ca_pem(), TestPki::new("a").ca_pem());
    }

    #[test]
    fn positive_write_to_round_trips() {
        let dir = crate::tempdir();
        let pki = TestPki::new("testkit CA");
        let (cert, key) = pki
            .issue(&LeafSpec::service("svc-a"))
            .write_to(&dir, "svc-a");
        assert!(std::fs::read_to_string(cert)
            .unwrap()
            .contains("BEGIN CERTIFICATE"));
        assert!(std::fs::read_to_string(key)
            .unwrap()
            .contains("PRIVATE KEY"));
        assert!(pki.write_ca(&dir, "root_ca").exists());
    }
}
