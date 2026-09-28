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

/// `UTCTime` and `GeneralizedTime` inside `Validity`.
const UTC_TIME: u8 = 0x17;
const GENERALIZED_TIME: u8 = 0x18;
/// `[0] EXPLICIT Version` at the head of `TBSCertificate`.
const VERSION: u8 = 0xa0;

/// A DER certificate's validity window `(notBefore, notAfter)` as unix seconds;
/// `None` when the structure or a time is malformed, or the window is inverted
/// (security-hardening S11b, `agent doctor`).
pub fn validity(der: &[u8]) -> Option<(u64, u64)> {
    let (tag, cert, trailing) = tlv(der)?;
    if tag != SEQUENCE || !trailing.is_empty() {
        return None;
    }
    let (tag, tbs, _) = tlv(cert)?;
    if tag != SEQUENCE {
        return None;
    }
    let fields = children(tbs)?;
    // version?, serialNumber, signature, issuer, validity, ...
    let skip = usize::from(fields.first()?.0 == VERSION);
    let (tag, validity) = *fields.get(skip + 3)?;
    if tag != SEQUENCE {
        return None;
    }
    let times = children(validity)?;
    if times.len() != 2 {
        return None;
    }
    let from = der_time(times[0].0, times[0].1)?;
    let until = der_time(times[1].0, times[1].1)?;
    (from <= until).then_some((from, until))
}

/// `YYMMDDHHMMSSZ` (UTCTime, RFC 5280: YY < 50 is 20YY) or `YYYYMMDDHHMMSSZ`
/// (GeneralizedTime) as unix seconds. Only the `Z` forms RFC 5280 allows.
fn der_time(tag: u8, raw: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(raw).ok()?;
    let digits = text.strip_suffix('Z')?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let num = |r: std::ops::Range<usize>| digits.get(r)?.parse::<u64>().ok();
    let (year, rest) = match (tag, digits.len()) {
        (UTC_TIME, 12) => {
            let yy = num(0..2)?;
            (if yy < 50 { 2000 + yy } else { 1900 + yy }, 2)
        }
        (GENERALIZED_TIME, 14) => (num(0..4)?, 4),
        _ => return None,
    };
    let month = num(rest..rest + 2)?;
    let day = num(rest + 2..rest + 4)?;
    let (h, m, sec) = (
        num(rest + 4..rest + 6)?,
        num(rest + 6..rest + 8)?,
        num(rest + 8..rest + 10)?,
    );
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || h > 23 || m > 59 || sec > 60 {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    Some(days * 86_400 + h * 3600 + m * 60 + sec)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (H. Hinnant's algorithm);
/// `None` before the epoch.
fn days_from_civil(year: u64, month: u64, day: u64) -> Option<u64> {
    let y = if month <= 2 {
        year.checked_sub(1)?
    } else {
        year
    };
    let era = y / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe).checked_sub(719_468)
}

/// The DER of every `CERTIFICATE` block in a PEM file, in order.
pub fn pem_certificates(pem: &[u8]) -> Vec<Vec<u8>> {
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";
    let Ok(text) = std::str::from_utf8(pem) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(BEGIN) {
        let body = &rest[start + BEGIN.len()..];
        let Some(end) = body.find(END) else {
            break;
        };
        let b64: String = body[..end].chars().filter(|c| !c.is_whitespace()).collect();
        if let Ok(der) = base64::engine::general_purpose::STANDARD.decode(b64) {
            out.push(der);
        }
        rest = &body[end + END.len()..];
    }
    out
}

/// The [`validity`] of the first certificate in a PEM file: a listener's or
/// client's leaf. Read with the TLS loader's size cap.
pub fn cert_file_validity(path: &std::path::Path) -> Result<(u64, u64), String> {
    let pem = crate::tls::read_pem(path)?;
    let first = pem_certificates(&pem)
        .into_iter()
        .next()
        .ok_or_else(|| format!("`{}` holds no CERTIFICATE block", path.display()))?;
    validity(&first).ok_or_else(|| format!("`{}` is not a readable certificate", path.display()))
}

#[cfg(test)]
mod tests;
