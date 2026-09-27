use agent_testkit::pki::{LeafSpec, TestPki};
use base64::engine::general_purpose::STANDARD;
use rstest::rstest;

use super::*;

/// A DER element with a definite length (short or long form).
fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    match content.len() {
        n @ 0..=0x7f => out.push(n as u8),
        n @ 0x80..=0xff => out.extend([0x81, n as u8]),
        n => out.extend([0x82, (n >> 8) as u8, n as u8]),
    }
    out.extend_from_slice(content);
    out
}

fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.concat()
}

/// An extension: `SEQ { OID, [BOOLEAN,] OCTET STRING }`.
fn ext(oid: &[u8], critical: bool, value: &[u8]) -> Vec<u8> {
    let mut parts = vec![der(OID, oid)];
    if critical {
        parts.push(der(BOOLEAN, &[0xff]));
    }
    parts.push(der(OCTET_STRING, value));
    der(SEQUENCE, &cat(&parts))
}

/// A SAN extension holding `names` (already-encoded GeneralName elements).
fn san(names: &[Vec<u8>]) -> Vec<u8> {
    ext(SAN_OID, false, &der(SEQUENCE, &cat(names)))
}

fn uri(s: &[u8]) -> Vec<u8> {
    der(URI_NAME, s)
}

/// A minimal certificate shape: `SEQ { SEQ { INTEGER, [3] { SEQ { exts } } }, SEQ {}, BIT STRING }`.
/// Unsigned: this exercises the reader, not a chain check (rustls did that).
fn cert(exts: &[Vec<u8>]) -> Vec<u8> {
    let tbs = der(
        SEQUENCE,
        &cat(&[
            der(0x02, &[0x01]),
            der(EXTENSIONS, &der(SEQUENCE, &cat(exts))),
        ]),
    );
    der(
        SEQUENCE,
        &cat(&[tbs, der(SEQUENCE, &[]), der(0x03, &[0x00])]),
    )
}

fn pem_der(pem: &str) -> Vec<u8> {
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<Vec<_>>()
        .concat();
    STANDARD.decode(body).expect("pem body")
}

#[rstest]
#[case::positive_service_leaf_carries_its_spiffe_id(
    LeafSpec::service("fleet"),
    vec!["spiffe://agent.test/svc/fleet"]
)]
#[case::corner_dns_only_leaf_has_no_uri(LeafSpec::dns_only("seam.internal"), vec![])]
fn issued_leaf_uris(#[case] spec: LeafSpec, #[case] want: Vec<&str>) {
    let pki = TestPki::new("peer test CA");
    let der = pem_der(&pki.issue(&spec).cert_pem);
    let peer = PeerCert::from_der(&der).expect("an issued leaf parses");
    assert_eq!(peer.uris, want);
    assert_eq!(peer.thumbprint, thumbprint(&der));
}

#[test]
fn corner_thumbprint_is_per_certificate() {
    let pki = TestPki::new("peer test CA");
    let a = pem_der(&pki.issue(&LeafSpec::service("a")).cert_pem);
    let b = pem_der(&pki.issue(&LeafSpec::service("a")).cert_pem);
    // Same SAN, different key and serial: a different certificate.
    assert_ne!(thumbprint(&a), thumbprint(&b));
    assert_eq!(thumbprint(&a), thumbprint(&a.clone()));
    // base64url of 32 bytes, unpadded.
    assert_eq!(thumbprint(&a).len(), 43);
}

#[rstest]
#[case::positive_one_uri(cert(&[san(&[uri(b"spiffe://x/svc/a")])]), Some(vec!["spiffe://x/svc/a"]))]
#[case::positive_critical_san(
    cert(&[ext(SAN_OID, true, &der(SEQUENCE, &uri(b"spiffe://x/svc/a")))]),
    Some(vec!["spiffe://x/svc/a"])
)]
#[case::positive_two_uris_in_order(
    cert(&[san(&[uri(b"spiffe://x/svc/a"), uri(b"spiffe://x/svc/b")])]),
    Some(vec!["spiffe://x/svc/a", "spiffe://x/svc/b"])
)]
#[case::corner_dns_entries_ignored(cert(&[san(&[der(0x82, b"host.internal")])]), Some(vec![]))]
#[case::corner_other_extension_only(cert(&[ext(&[0x55, 0x1d, 0x0f], true, &[0x03, 0x02, 0x05, 0xa0])]), Some(vec![]))]
#[case::corner_no_extensions(
    der(SEQUENCE, &cat(&[der(SEQUENCE, &der(0x02, &[1])), der(SEQUENCE, &[])])),
    Some(vec![])
)]
#[case::boundary_uri_at_the_cap(
    cert(&[san(&[uri(&vec![b'a'; MAX_URI_BYTES])])]),
    Some(vec![std::str::from_utf8(&[b'a'; MAX_URI_BYTES]).unwrap()])
)]
#[case::boundary_uri_over_the_cap_is_skipped(cert(&[san(&[uri(&vec![b'a'; MAX_URI_BYTES + 1])])]), Some(vec![]))]
#[case::boundary_max_entries(cert(&[san(&vec![der(0x82, b"h.internal"); MAX_SAN_ENTRIES])]), Some(vec![]))]
#[case::adversarial_too_many_entries(cert(&[san(&vec![der(0x82, b"h.internal"); MAX_SAN_ENTRIES + 1])]), None)]
#[case::adversarial_second_san_extension(
    cert(&[san(&[uri(b"spiffe://x/svc/a")]), san(&[uri(b"spiffe://x/svc/operator")])]),
    None
)]
#[case::adversarial_control_char_uri_skipped(
    cert(&[san(&[uri(b"spiffe://x/svc/a\n"), uri(b"spiffe://x/svc/b")])]),
    Some(vec!["spiffe://x/svc/b"])
)]
#[case::adversarial_space_in_uri_skipped(cert(&[san(&[uri(b"spiffe://x/svc/a b")])]), Some(vec![]))]
#[case::adversarial_non_ascii_uri_skipped(cert(&[san(&[uri("spiffe://x/svc/\u{e9}".as_bytes())])]), Some(vec![]))]
#[case::adversarial_empty_uri_skipped(cert(&[san(&[uri(b"")])]), Some(vec![]))]
#[case::adversarial_empty_input(vec![], None)]
#[case::adversarial_not_a_sequence(der(OCTET_STRING, &[1, 2, 3]), None)]
#[case::adversarial_trailing_bytes(cat(&[cert(&[san(&[uri(b"spiffe://x/svc/a")])]), vec![0x00]]), None)]
#[case::adversarial_indefinite_length(vec![0x30, 0x80, 0x00, 0x00], None)]
#[case::adversarial_length_past_the_end(vec![0x30, 0x10, 0x30, 0x00], None)]
#[case::adversarial_non_minimal_long_length(vec![0x30, 0x81, 0x02, 0x30, 0x00], None)]
#[case::adversarial_leading_zero_length(vec![0x30, 0x82, 0x00, 0x82, 0x30, 0x00], None)]
#[case::adversarial_four_byte_length(vec![0x30, 0x84, 0x00, 0x00, 0x00, 0x02, 0x30, 0x00], None)]
#[case::adversarial_high_tag_number(vec![0x1f, 0x81, 0x00, 0x00], None)]
#[case::adversarial_extension_missing_value(cert(&[der(SEQUENCE, &der(OID, SAN_OID))]), None)]
#[case::adversarial_extension_not_a_sequence(cert(&[der(OCTET_STRING, &[0])]), None)]
#[case::adversarial_san_value_not_a_sequence(cert(&[ext(SAN_OID, false, &uri(b"spiffe://x/svc/a"))]), None)]
#[case::adversarial_truncated_san(cert(&[ext(SAN_OID, false, &[0x30, 0x05, 0x86, 0x01])]), None)]
fn san_uri_cases(#[case] der: Vec<u8>, #[case] want: Option<Vec<&str>>) {
    let got = san_uris(&der);
    assert_eq!(
        got,
        want.map(|v| v.into_iter().map(str::to_string).collect::<Vec<_>>())
    );
}

#[test]
fn adversarial_every_truncation_of_a_real_leaf_is_refused_without_panicking() {
    let pki = TestPki::new("peer test CA");
    let der = pem_der(&pki.issue(&LeafSpec::service("fleet")).cert_pem);
    for cut in 0..der.len() {
        assert_eq!(san_uris(&der[..cut]), None, "truncated at {cut}");
    }
}
