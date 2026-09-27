//! Issuer-profile tables (security-hardening S3): resolving `[[auth.issuers]]`
//! entries, and mapping verified claims to an identity. Pure — no keys, no network.

use rstest::rstest;
use serde_json::{json, Value};

use super::{
    ClaimRejection, KeySource, Profile, ResolvedIssuer, ENTRA_JWKS, GOOGLE_ISSUERS, GOOGLE_JWKS,
};
use crate::server::auth::IssuerParams;

fn google() -> IssuerParams {
    IssuerParams {
        name: "google".into(),
        profile: "google".into(),
        audience: "client-1.apps.googleusercontent.com".into(),
        allowed_domains: vec!["example.com".into()],
        ..IssuerParams::default()
    }
}

fn entra() -> IssuerParams {
    IssuerParams {
        name: "entra".into(),
        profile: "entra".into(),
        audience: "api://agent".into(),
        allowed_tenants: vec!["11111111-2222-3333-4444-555555555555".into()],
        ..IssuerParams::default()
    }
}

fn generic() -> IssuerParams {
    IssuerParams {
        name: "keycloak".into(),
        profile: "generic".into(),
        issuer: "https://idp.example/realms/agents".into(),
        audience: "agent-seddon".into(),
        ..IssuerParams::default()
    }
}

#[rstest]
#[case::positive_google_defaults(google(), None)]
#[case::positive_entra_defaults(entra(), None)]
#[case::positive_generic_discovery(generic(), None)]
#[case::positive_generic_fixed_jwks(IssuerParams { jwks_url: "https://idp.example/certs".into(), ..generic() }, None)]
#[case::positive_generic_loopback_test_issuer(IssuerParams { issuer: "http://127.0.0.1:8123".into(), ..generic() }, None)]
#[case::positive_empty_profile_is_generic(IssuerParams { profile: String::new(), ..generic() }, None)]
#[case::negative_unknown_profile(IssuerParams { profile: "okta".into(), ..generic() }, Some("unknown issuer profile"))]
#[case::negative_missing_audience(IssuerParams { audience: " ".into(), ..google() }, Some("needs `audience`"))]
#[case::negative_google_admits_nobody(IssuerParams { allowed_domains: vec![], ..google() }, Some("`allowed_domains`"))]
#[case::negative_entra_without_tenants(IssuerParams { allowed_tenants: vec![], ..entra() }, Some("`allowed_tenants`"))]
#[case::negative_generic_without_issuer(IssuerParams { issuer: String::new(), ..generic() }, Some("needs `issuer`"))]
#[case::corner_google_consumer_only_deploy(IssuerParams { allowed_domains: vec![], default_tenant: "home".into(), ..google() }, None)]
#[case::corner_entra_default_tenant_refused(IssuerParams { default_tenant: "x".into(), ..entra() }, Some("`default_tenant` does not apply"))]
#[case::corner_allowed_tenants_outside_entra(IssuerParams { allowed_tenants: vec!["t".into()], ..generic() }, Some("entra` profile only"))]
#[case::boundary_empty_allowed_domain_entry(IssuerParams { allowed_domains: vec![String::new()], ..google() }, Some("empty or unsafe"))]
#[case::adversarial_google_trusts_roles(IssuerParams { trust_roles_claim: Some(true), ..google() }, Some("carry no roles"))]
#[case::adversarial_google_tenant_claim_override(IssuerParams { tenant_claim: "email".into(), ..google() }, Some("fixes `tenant_claim`"))]
#[case::adversarial_entra_subject_claim_override(IssuerParams { subject_claim: "email".into(), ..entra() }, Some("fixes `subject_claim`"))]
#[case::adversarial_name_traversal(IssuerParams { name: "../x".into(), ..generic() }, Some("plain identifier"))]
#[case::adversarial_unsafe_default_tenant(IssuerParams { default_tenant: "a/b".into(), ..generic() }, Some("not a safe segment"))]
#[case::adversarial_unsafe_allowed_domain(IssuerParams { allowed_domains: vec!["../etc".into()], ..google() }, Some("empty or unsafe"))]
#[case::adversarial_jwks_plain_http_remote(IssuerParams { jwks_url: "http://idp.example/certs".into(), ..generic() }, Some("must use https"))]
#[case::adversarial_discovery_plain_http_remote(IssuerParams { issuer: "http://idp.example".into(), ..generic() }, Some("must use https"))]
#[case::adversarial_jwks_embedded_credentials(IssuerParams { jwks_url: "https://u:p@idp.example/certs".into(), ..google() }, Some("credentials"))]
#[case::adversarial_jwks_file_scheme(IssuerParams { jwks_url: "file:///etc/jwks.json".into(), ..entra() }, Some("must use https"))]
fn resolve_cases(#[case] params: IssuerParams, #[case] want_err: Option<&str>) {
    match (ResolvedIssuer::resolve(&params), want_err) {
        (Ok(_), None) => {}
        (Err(e), Some(want)) => assert!(e.contains(want), "want {want:?} in {e:?}"),
        (got, want) => panic!("{params:?}: got {got:?}, want error {want:?}"),
    }
}

#[rstest]
#[case::google(google(), Profile::Google, GOOGLE_ISSUERS.to_vec(), KeySource::Jwks(GOOGLE_JWKS.into()))]
#[case::entra(
    entra(),
    Profile::Entra,
    vec!["https://login.microsoftonline.com/11111111-2222-3333-4444-555555555555/v2.0"],
    KeySource::Jwks(ENTRA_JWKS.into())
)]
#[case::generic_discovery(
    generic(),
    Profile::Generic,
    vec!["https://idp.example/realms/agents"],
    KeySource::Discovery("https://idp.example/realms/agents".into())
)]
#[case::google_issuer_override_for_a_test_issuer(
    IssuerParams { issuer: "http://127.0.0.1:9".into(), jwks_url: "http://127.0.0.1:9/jwks".into(), ..google() },
    Profile::Google,
    vec!["http://127.0.0.1:9"],
    KeySource::Jwks("http://127.0.0.1:9/jwks".into())
)]
fn positive_resolved_defaults(
    #[case] params: IssuerParams,
    #[case] profile: Profile,
    #[case] iss: Vec<&str>,
    #[case] keys: KeySource,
) {
    let r = ResolvedIssuer::resolve(&params).expect("resolves");
    assert_eq!(r.profile, profile);
    assert_eq!(r.accepted_iss, iss);
    assert_eq!(r.keys, keys);
}

/// (tenant, subject, roles, email) on success.
type Want = Result<
    (
        &'static str,
        &'static str,
        Vec<&'static str>,
        Option<&'static str>,
    ),
    ClaimRejection,
>;

fn with(base: IssuerParams, edit: impl FnOnce(&mut IssuerParams)) -> IssuerParams {
    let mut p = base;
    edit(&mut p);
    p
}

#[rstest]
// --- google ---
#[case::positive_google_hd_is_tenant(
    google(),
    json!({"sub": "g-1", "hd": "example.com", "email": "Ann@Example.com", "email_verified": true}),
    Ok(("example.com", "g-1", vec![], Some("ann@example.com")))
)]
#[case::negative_google_email_unverified(
    google(),
    json!({"sub": "g-1", "hd": "example.com", "email": "a@example.com", "email_verified": false}),
    Err(ClaimRejection::EmailNotVerified)
)]
#[case::negative_google_email_verified_absent(
    google(),
    json!({"sub": "g-1", "hd": "example.com", "email": "a@example.com"}),
    Err(ClaimRejection::EmailNotVerified)
)]
#[case::negative_google_domain_not_allowed(
    google(),
    json!({"sub": "g-1", "hd": "other.com", "email": "a@other.com", "email_verified": true}),
    Err(ClaimRejection::DomainNotAllowed)
)]
#[case::negative_google_consumer_account_without_hd(
    google(),
    json!({"sub": "g-1", "email": "a@gmail.com", "email_verified": true}),
    Err(ClaimRejection::MissingTenant)
)]
#[case::corner_google_default_tenant_for_consumer(
    with(google(), |p| p.default_tenant = "home".into()),
    json!({"sub": "g-1", "email": "a@gmail.com", "email_verified": true}),
    Ok(("home", "g-1", vec![], Some("a@gmail.com")))
)]
#[case::corner_google_string_true_verified(
    google(),
    json!({"sub": "g-1", "hd": "example.com", "email": "a@example.com", "email_verified": "true"}),
    Ok(("example.com", "g-1", vec![], Some("a@example.com")))
)]
#[case::boundary_google_hd_case_folded(
    google(),
    json!({"sub": "g-1", "hd": "EXAMPLE.COM", "email": "a@example.com", "email_verified": true}),
    Ok(("example.com", "g-1", vec![], Some("a@example.com")))
)]
#[case::adversarial_google_roles_claim_ignored(
    google(),
    json!({"sub": "g-1", "hd": "example.com", "email": "a@example.com", "email_verified": true, "roles": ["operator"]}),
    Ok(("example.com", "g-1", vec![], Some("a@example.com")))
)]
#[case::adversarial_google_disallowed_hd_not_rescued_by_default(
    with(google(), |p| p.default_tenant = "home".into()),
    json!({"sub": "g-1", "hd": "evil.com", "email": "a@evil.com", "email_verified": true}),
    Err(ClaimRejection::DomainNotAllowed)
)]
// --- entra ---
#[case::positive_entra_tid_is_tenant_oid_is_subject(
    entra(),
    json!({"sub": "pairwise", "oid": "o-1", "tid": "11111111-2222-3333-4444-555555555555"}),
    Ok(("11111111-2222-3333-4444-555555555555", "o-1", vec![], None))
)]
#[case::positive_entra_trusted_app_roles(
    with(entra(), |p| p.trust_roles_claim = Some(true)),
    json!({"oid": "o-1", "tid": "11111111-2222-3333-4444-555555555555", "roles": ["reviewer"]}),
    Ok(("11111111-2222-3333-4444-555555555555", "o-1", vec!["reviewer"], None))
)]
#[case::negative_entra_foreign_directory(
    entra(),
    json!({"oid": "o-1", "tid": "99999999-0000-0000-0000-000000000000"}),
    Err(ClaimRejection::TenantNotAllowed)
)]
#[case::corner_entra_sub_is_not_the_subject(
    entra(),
    json!({"sub": "pairwise", "tid": "11111111-2222-3333-4444-555555555555"}),
    Err(ClaimRejection::MissingSubject)
)]
// --- generic ---
#[case::positive_generic_org_claim(
    with(generic(), |p| p.trust_roles_claim = Some(true)),
    json!({"sub": "u-1", "org": "acme", "roles": ["reviewer", 7]}),
    Ok(("acme", "u-1", vec!["reviewer"], None))
)]
#[case::positive_generic_custom_claims(
    with(generic(), |p| { p.tenant_claim = "team".into(); p.subject_claim = "preferred_username".into(); }),
    json!({"sub": "x", "preferred_username": "ann", "team": "blue"}),
    Ok(("blue", "ann", vec![], None))
)]
#[case::negative_generic_missing_tenant(
    generic(),
    json!({"sub": "u-1"}),
    Err(ClaimRejection::MissingTenant)
)]
#[case::negative_generic_roles_untrusted_by_default(
    generic(),
    json!({"sub": "u-1", "org": "acme", "roles": ["operator"]}),
    Ok(("acme", "u-1", vec![], None))
)]
#[case::negative_generic_email_domain_not_allowed(
    with(generic(), |p| p.allowed_domains = vec!["example.com".into()]),
    json!({"sub": "u-1", "org": "acme", "email": "a@other.com"}),
    Err(ClaimRejection::DomainNotAllowed)
)]
#[case::negative_generic_required_verification(
    with(generic(), |p| p.require_email_verified = true),
    json!({"sub": "u-1", "org": "acme", "email": "a@example.com", "email_verified": false}),
    Err(ClaimRejection::EmailNotVerified)
)]
#[case::corner_generic_default_tenant(
    with(generic(), |p| p.default_tenant = "solo".into()),
    json!({"sub": "u-1"}),
    Ok(("solo", "u-1", vec![], None))
)]
#[case::boundary_generic_empty_tenant_claim_is_absent(
    generic(),
    json!({"sub": "u-1", "org": ""}),
    Err(ClaimRejection::MissingTenant)
)]
#[case::boundary_generic_empty_subject(
    generic(),
    json!({"sub": " ", "org": "acme"}),
    Err(ClaimRejection::MissingSubject)
)]
#[case::adversarial_generic_traversal_tenant(
    generic(),
    json!({"sub": "u-1", "org": "../other"}),
    Err(ClaimRejection::UnsafeTenant)
)]
#[case::adversarial_generic_domain_suffix_trick(
    with(generic(), |p| p.allowed_domains = vec!["example.com".into()]),
    json!({"sub": "u-1", "org": "acme", "email": "a@evil-example.com"}),
    Err(ClaimRejection::DomainNotAllowed)
)]
#[case::adversarial_generic_double_at(
    with(generic(), |p| p.allowed_domains = vec!["example.com".into()]),
    json!({"sub": "u-1", "org": "acme", "email": "a@example.com@evil.com"}),
    Err(ClaimRejection::DomainNotAllowed)
)]
#[case::adversarial_generic_domain_rule_without_email(
    with(generic(), |p| p.allowed_domains = vec!["example.com".into()]),
    json!({"sub": "u-1", "org": "acme"}),
    Err(ClaimRejection::DomainNotAllowed)
)]
#[case::adversarial_generic_non_string_tenant(
    generic(),
    json!({"sub": "u-1", "org": ["acme", "other"]}),
    Err(ClaimRejection::MissingTenant)
)]
fn identity_cases(#[case] params: IssuerParams, #[case] claims: Value, #[case] want: Want) {
    let issuer = ResolvedIssuer::resolve(&params).expect("fixture resolves");
    let got = issuer.identity(&claims);
    match (got, want) {
        (Ok(id), Ok((tenant, subject, roles, email))) => {
            assert_eq!(id.tenant, tenant, "tenant");
            assert_eq!(id.subject, subject, "subject");
            assert_eq!(id.roles, roles, "roles");
            assert_eq!(id.email.as_deref(), email, "email");
            assert_eq!(id.issuer, params.name, "issuer name");
        }
        (Err(got), Err(want)) => assert_eq!(got, want),
        (got, want) => panic!("claims {claims}: got {got:?}, want {want:?}"),
    }
}
