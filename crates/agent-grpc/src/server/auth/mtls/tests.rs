use rstest::rstest;

use super::*;

const FLEET: &str = "spiffe://agent.test/svc/fleet";
const SEAM: &str = "spiffe://agent.test/svc/seam";

fn entry(san: &str, service: &str, tenant: &str, roles: &[&str]) -> MtlsBindingParams {
    MtlsBindingParams {
        san: san.into(),
        service: service.into(),
        tenant: tenant.into(),
        roles: roles.iter().map(|r| (*r).to_string()).collect(),
    }
}

fn fleet() -> MtlsBindingParams {
    entry(FLEET, "fleet", "acme", &["svc_fleet"])
}

fn peer(uris: &[&str], thumbprint: &str) -> PeerCert {
    PeerCert {
        uris: uris.iter().map(|u| (*u).to_string()).collect(),
        thumbprint: thumbprint.into(),
    }
}

#[rstest]
#[case::positive_one(vec![fleet()], None)]
#[case::positive_two(vec![fleet(), entry(SEAM, "seam", "acme", &["svc_seam"])], None)]
#[case::corner_none(vec![], None)]
#[case::corner_trimmed(vec![entry(" spiffe://agent.test/svc/fleet ", " fleet ", " acme ", &[" svc_fleet "])], None)]
#[case::negative_dns_san(vec![entry("fleet.internal", "fleet", "acme", &["svc_fleet"])], Some("spiffe://"))]
#[case::negative_https_uri(vec![entry("https://agent.test/svc/fleet", "fleet", "acme", &["svc_fleet"])], Some("spiffe://"))]
#[case::negative_bare_scheme(vec![entry("spiffe://", "fleet", "acme", &["svc_fleet"])], Some("spiffe://"))]
#[case::negative_no_roles(vec![entry(FLEET, "fleet", "acme", &[])], Some("roles"))]
#[case::negative_same_san_twice(vec![fleet(), entry(FLEET, "other", "acme", &["svc_seam"])], Some("bound twice"))]
#[case::negative_same_service_twice(vec![fleet(), entry(SEAM, "fleet", "acme", &["svc_seam"])], Some("bound twice"))]
#[case::boundary_role_cap(vec![entry(FLEET, "fleet", "acme", &["r"; MAX_MTLS_ROLES])], None)]
#[case::boundary_over_role_cap(vec![entry(FLEET, "fleet", "acme", &["r"; MAX_MTLS_ROLES + 1])], Some("roles"))]
#[case::adversarial_traversal_service(vec![entry(FLEET, "../operator", "acme", &["svc_fleet"])], Some("service"))]
#[case::adversarial_traversal_tenant(vec![entry(FLEET, "fleet", "../other", &["svc_fleet"])], Some("tenant"))]
#[case::adversarial_space_in_san(vec![entry("spiffe://agent.test/svc/fleet x", "fleet", "acme", &["svc_fleet"])], Some("spiffe://"))]
#[case::adversarial_newline_in_san(vec![entry("spiffe://agent.test/svc/\nfleet", "fleet", "acme", &["svc_fleet"])], Some("spiffe://"))]
#[case::adversarial_unsafe_role(vec![entry(FLEET, "fleet", "acme", &["svc/fleet"])], Some("role"))]
#[case::adversarial_huge_san(vec![entry(&format!("spiffe://{}", "a".repeat(4096)), "fleet", "acme", &["svc_fleet"])], Some("spiffe://"))]
fn parse_cases(#[case] entries: Vec<MtlsBindingParams>, #[case] err: Option<&str>) {
    match (MtlsBindings::parse(&entries), err) {
        (Ok(_), None) => {}
        (Err(e), Some(want)) => assert!(e.contains(want), "{e}"),
        (got, want) => panic!("got {got:?}, want error containing {want:?}"),
    }
}

#[test]
fn boundary_binding_count_cap() {
    let many = |n: usize| -> Vec<MtlsBindingParams> {
        (0..n)
            .map(|i| {
                entry(
                    &format!("spiffe://agent.test/svc/s{i}"),
                    &format!("s{i}"),
                    "acme",
                    &["svc_seam"],
                )
            })
            .collect()
    };
    assert!(MtlsBindings::parse(&many(MAX_MTLS_BINDINGS)).is_ok());
    assert!(MtlsBindings::parse(&many(MAX_MTLS_BINDINGS + 1)).is_err());
}

#[rstest]
#[case::positive_bound_san(&[FLEET], Some("fleet"))]
#[case::positive_bound_among_others(&["spiffe://agent.test/svc/other", FLEET], Some("fleet"))]
#[case::negative_unbound_san(&["spiffe://agent.test/svc/other"], None)]
#[case::corner_no_uris(&[], None)]
#[case::adversarial_two_bound_sans_are_ambiguous(&[FLEET, SEAM], None)]
#[case::adversarial_case_differs(&["SPIFFE://agent.test/svc/fleet"], None)]
#[case::adversarial_prefix_of_a_bound_san(&["spiffe://agent.test/svc/flee"], None)]
fn service_of_cases(#[case] uris: &[&str], #[case] want: Option<&str>) {
    let b = MtlsBindings::parse(&[fleet(), entry(SEAM, "seam", "acme", &["svc_seam"])]).unwrap();
    let got = b.service_of(&peer(uris, "t")).map(|s| s.service.as_str());
    assert_eq!(got, want);
}

#[test]
fn positive_subject_is_namespaced() {
    let b = MtlsBindings::parse(&[fleet()]).unwrap();
    assert_eq!(
        b.service_of(&peer(&[FLEET], "t")).unwrap().subject(),
        "svc:fleet"
    );
}

#[rstest]
#[case::positive_person_token_needs_no_cert(None, None, true)]
#[case::positive_person_token_over_any_cert(None, Some(peer(&["spiffe://x/y"], "zz")), true)]
#[case::positive_bound_cert_presented(Some("tp"), Some(peer(&[FLEET], "tp")), true)]
#[case::positive_relayed_by_a_known_service(Some("tp"), Some(peer(&[SEAM], "other")), true)]
#[case::negative_no_cert(Some("tp"), None, false)]
#[case::adversarial_unbound_cert_from_the_same_ca(Some("tp"), Some(peer(&["spiffe://agent.test/svc/laptop"], "other")), false)]
#[case::adversarial_cert_without_uris(Some("tp"), Some(peer(&[], "other")), false)]
#[case::adversarial_ambiguous_relay(Some("tp"), Some(peer(&[FLEET, SEAM], "other")), false)]
#[case::corner_bound_thumbprint_even_if_unbound_san(Some("tp"), Some(peer(&["spiffe://x/y"], "tp")), true)]
fn cnf_cases(#[case] cnf: Option<&str>, #[case] p: Option<PeerCert>, #[case] want: bool) {
    let b = MtlsBindings::parse(&[fleet(), entry(SEAM, "seam", "acme", &["svc_seam"])]).unwrap();
    assert_eq!(cnf_allows(cnf, p.as_ref(), &b), want);
}
