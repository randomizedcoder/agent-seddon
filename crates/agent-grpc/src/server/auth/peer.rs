//! The client certificate a mutual-TLS peer presented (security-hardening S10,
//! docs/design/security-hardening/07-transport-tls-and-pki.md).
//!
//! rustls has already verified the chain against `[grpc.tls] client_ca` before a
//! request reaches the layer, so the certificate is CA-signed; this module only reads
//! what the service identity needs from the leaf:
//!
//! - its URI subject alternative names (the `spiffe://…/svc/<name>` ids that
//!   `[auth.mtls] bindings` map to a service principal), and
//! - its RFC 8705 `x5t#S256` thumbprint, the value a service token's `cnf` claim
//!   binds it to.
//!
//! The DER reader is deliberately small and fails closed: an indefinite or
//! over-long length, a length past the end of its input, a second SAN extension, or
//! more SAN entries than [`MAX_SAN_ENTRIES`] yields no identity at all.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use tonic::codegen::http;
use tonic::transport::server::{TcpConnectInfo, TlsConnectInfo};

/// SAN entries read from one certificate; a real service leaf has a handful.
pub const MAX_SAN_ENTRIES: usize = 64;
/// A URI SAN longer than this is not a service id.
pub const MAX_URI_BYTES: usize = 2048;

/// What the layer knows about a verified mutual-TLS peer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerCert {
    /// URI SANs, in certificate order (ASCII, no control characters).
    pub uris: Vec<String>,
    /// base64url (no padding) SHA-256 of the leaf's DER: RFC 8705 `x5t#S256`.
    pub thumbprint: String,
}

impl PeerCert {
    /// Read a leaf certificate. `None` when it is not DER this reader accepts.
    pub fn from_der(der: &[u8]) -> Option<Self> {
        Some(Self {
            uris: san_uris(der)?,
            thumbprint: thumbprint(der),
        })
    }
}

/// RFC 8705 `x5t#S256` of a DER certificate.
pub fn thumbprint(der: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(ring::digest::digest(&ring::digest::SHA256, der))
}

/// The verified client certificate on this request's connection, when the listener
/// is mutual TLS and the peer presented one. tonic puts the connection's
/// `TlsConnectInfo` in the request extensions; a plaintext or server-only TLS
/// connection has no peer certificates.
pub fn of_request<B>(req: &http::Request<B>) -> Option<PeerCert> {
    let info = req.extensions().get::<TlsConnectInfo<TcpConnectInfo>>()?;
    of_certs(info.peer_certs()?.as_slice())
}

/// The leaf (first) certificate of a presented chain.
pub fn of_certs(chain: &[tonic::transport::CertificateDer<'static>]) -> Option<PeerCert> {
    PeerCert::from_der(chain.first()?.as_ref())
}

/// One DER element: its tag, its content, and what follows it.
fn tlv(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    // Multi-byte (high) tag numbers never appear in the structures read here.
    if tag & 0x1f == 0x1f {
        return None;
    }
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = match first {
        0x00..=0x7f => (usize::from(first), rest),
        // 0x80 is the indefinite form (BER only); more than three length bytes is
        // far larger than any certificate.
        0x81..=0x83 => {
            let n = usize::from(first & 0x7f);
            if rest.len() < n {
                return None;
            }
            let (bytes, rest) = rest.split_at(n);
            // DER: the long form only when the short one cannot hold it, and no
            // leading zero byte.
            if bytes[0] == 0 {
                return None;
            }
            let len = bytes
                .iter()
                .fold(0usize, |acc, b| (acc << 8) | usize::from(*b));
            if len < 0x80 {
                return None;
            }
            (len, rest)
        }
        _ => return None,
    };
    if rest.len() < len {
        return None;
    }
    let (value, rest) = rest.split_at(len);
    Some((tag, value, rest))
}

/// Every element of a constructed value's content, in order; `None` if any is
/// malformed.
fn children(mut content: &[u8]) -> Option<Vec<(u8, &[u8])>> {
    let mut out = Vec::new();
    while !content.is_empty() {
        let (tag, value, rest) = tlv(content)?;
        out.push((tag, value));
        content = rest;
    }
    Some(out)
}

const SEQUENCE: u8 = 0x30;
const OID: u8 = 0x06;
const BOOLEAN: u8 = 0x01;
const OCTET_STRING: u8 = 0x04;
/// `[3] EXPLICIT Extensions` inside `TBSCertificate`.
const EXTENSIONS: u8 = 0xa3;
/// `uniformResourceIdentifier [6] IA5String` inside `GeneralNames`.
const URI_NAME: u8 = 0x86;
/// id-ce-subjectAltName, 2.5.29.17.
const SAN_OID: &[u8] = &[0x55, 0x1d, 0x11];

/// The URI SANs of a DER certificate. `Some(vec![])` for a certificate with no SAN
/// extension or no URI entries; `None` when the structure is malformed, the SAN
/// extension appears twice, or it holds more than [`MAX_SAN_ENTRIES`] entries.
/// A URI entry that is not printable ASCII or is over [`MAX_URI_BYTES`] is skipped:
/// it can never equal a configured binding.
pub fn san_uris(der: &[u8]) -> Option<Vec<String>> {
    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signature }
    let (tag, cert, trailing) = tlv(der)?;
    if tag != SEQUENCE || !trailing.is_empty() {
        return None;
    }
    let (tag, tbs, _) = tlv(cert)?;
    if tag != SEQUENCE {
        return None;
    }
    let Some(&(_, exts)) = children(tbs)?.iter().find(|(t, _)| *t == EXTENSIONS) else {
        return Some(Vec::new());
    };
    let (tag, exts, trailing) = tlv(exts)?;
    if tag != SEQUENCE || !trailing.is_empty() {
        return None;
    }
    let mut san: Option<&[u8]> = None;
    for (tag, ext) in children(exts)? {
        if tag != SEQUENCE {
            return None;
        }
        // Extension ::= SEQUENCE { extnID OID, critical BOOLEAN DEFAULT FALSE, extnValue OCTET STRING }
        let parts = children(ext)?;
        let (oid, value) = match parts.as_slice() {
            [(OID, oid), (OCTET_STRING, v)] | [(OID, oid), (BOOLEAN, _), (OCTET_STRING, v)] => {
                (*oid, *v)
            }
            _ => return None,
        };
        if oid == SAN_OID {
            if san.is_some() {
                return None;
            }
            san = Some(value);
        }
    }
    let Some(san) = san else {
        return Some(Vec::new());
    };
    let (tag, names, trailing) = tlv(san)?;
    if tag != SEQUENCE || !trailing.is_empty() {
        return None;
    }
    let names = children(names)?;
    if names.len() > MAX_SAN_ENTRIES {
        return None;
    }
    Some(
        names
            .into_iter()
            .filter(|(t, _)| *t == URI_NAME)
            .filter(|(_, v)| {
                v.len() <= MAX_URI_BYTES && !v.is_empty() && v.iter().all(u8::is_ascii_graphic)
            })
            .filter_map(|(_, v)| std::str::from_utf8(v).ok().map(str::to_string))
            .collect(),
    )
}

#[cfg(test)]
mod tests;
