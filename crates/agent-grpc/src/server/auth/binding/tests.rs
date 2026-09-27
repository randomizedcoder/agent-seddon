//! Table-driven tests for role bindings: subject validation (adversarial
//! subjects, ids and role lists), matching (verified email only, domain
//! boundaries, tenant firewall, expiry), `operator_subjects` parsing, role
//! resolution, the permission-management rules and the last-admin guard, and the
//! store round trip. Hermetic: the in-memory config-store backend and a fixed
//! clock.

use std::sync::Arc;

use agent_config_store::{Backend, MemoryBackend, Write};
use agent_core::{
    RoleDef, ROLE_ACCESS_ADMIN, ROLE_AGENT_USER, ROLE_FLEET_ADMIN, ROLE_ORG_ADMIN, ROLE_REVIEWER,
    ROLE_VIEWER,
};
use rstest::rstest;

use super::*;

const T0: u64 = 1_700_000_000;

struct Fixed(u64);
impl Clock for Fixed {
    fn now_secs(&self) -> u64 {
        self.0
    }
}

fn binding(tenant: &str, kind: SubjectKind, subject: &str, roles: &[&str]) -> RoleBinding {
    RoleBinding {
        id: "b1".into(),
        tenant: tenant.into(),
        kind,
        subject: subject.into(),
        roles: roles.iter().map(ToString::to_string).collect(),
        granted_by: "user:google/root".into(),
        granted_at: T0,
        expires_at: 0,
    }
}

fn expiring(mut b: RoleBinding, at: u64) -> RoleBinding {
    b.expires_at = at;
    b
}

fn who<'a>(tenant: &'a str, subject: &'a str, email: Option<&'a str>, verified: bool) -> Who<'a> {
    Who {
        tenant,
        subject,
        email,
        email_verified: verified,
    }
}

fn principal(tenant: &str, subject: &str, roles: &[&str]) -> VerifiedPrincipal {
    VerifiedPrincipal {
        tenant: tenant.into(),
        subject: subject.into(),
        roles: roles.iter().map(ToString::to_string).collect(),
    }
}

/// The built-ins plus `global_reader` (read everything, every tenant) and
/// `exec_only` (a tenant role holding just `(use, exec)`).
fn catalog() -> RoleCatalog {
    let mut c = RoleCatalog::builtin();
    c.insert(
        "global_reader",
        RoleDef::actions_on_all(true, [Action::Read]),
    );
    c.insert(
        "exec_only",
        RoleDef::pairs(false, [(Action::Use, ResourceType::Exec)]),
    );
    c
}

fn sanitized(mut b: RoleBinding) -> RoleBinding {
    b.sanitize();
    b
}

// --- subject kinds ----------------------------------------------------------

#[rstest]
#[case::positive_sub("sub", Some(SubjectKind::Sub))]
#[case::positive_email("email", Some(SubjectKind::Email))]
#[case::positive_domain("domain", Some(SubjectKind::Domain))]
#[case::positive_mtls("mtls_san", Some(SubjectKind::MtlsSan))]
#[case::negative_unknown("group", None)]
#[case::corner_case_sensitive("Email", None)]
#[case::boundary_empty("", None)]
#[case::adversarial_padded(" sub", None)]
fn subject_kind_parse_cases(#[case] s: &str, #[case] want: Option<SubjectKind>) {
    assert_eq!(SubjectKind::parse(s), want);
    if let Some(k) = want {
        assert_eq!(k.as_str(), s, "as_str round-trips");
    }
}

// --- validation ---------------------------------------------------------------

fn roles_n(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("r{i}")).collect()
}

#[rstest]
#[case::positive_email(binding("acme", SubjectKind::Email, "alice@acme.com", &["viewer"]), true)]
#[case::positive_domain(binding("acme", SubjectKind::Domain, "acme.com", &["viewer"]), true)]
#[case::positive_sub(binding("acme", SubjectKind::Sub, "google/1234567890", &["viewer"]), true)]
#[case::positive_mtls(binding("acme", SubjectKind::MtlsSan, "spiffe://agent/svc/fleet", &["svc_fleet"]), true)]
// desc: sanitize trims and lowercases, so a pasted address still validates.
#[case::corner_email_trimmed_and_lowercased(binding("acme", SubjectKind::Email, "  Alice@ACME.com ", &["viewer"]), true)]
#[case::corner_tenant_with_dot(binding("example.com", SubjectKind::Domain, "example.com", &["viewer"]), true)]
#[case::negative_no_roles(binding("acme", SubjectKind::Email, "alice@acme.com", &[]), false)]
#[case::negative_email_without_at(binding("acme", SubjectKind::Email, "alice.acme.com", &["viewer"]), false)]
#[case::negative_domain_without_dot(binding("acme", SubjectKind::Domain, "localhost", &["viewer"]), false)]
#[case::negative_sub_without_issuer(binding("acme", SubjectKind::Sub, "1234567890", &["viewer"]), false)]
#[case::negative_sub_empty_idp_sub(binding("acme", SubjectKind::Sub, "google/", &["viewer"]), false)]
#[case::negative_empty_subject(binding("acme", SubjectKind::Email, "   ", &["viewer"]), false)]
#[case::adversarial_two_ats(binding("acme", SubjectKind::Email, "alice@evil.com@acme.com", &["viewer"]), false)]
#[case::adversarial_domain_leading_dot(binding("acme", SubjectKind::Domain, ".acme.com", &["viewer"]), false)]
#[case::adversarial_domain_double_dot(binding("acme", SubjectKind::Domain, "acme..com", &["viewer"]), false)]
#[case::adversarial_domain_wildcard(binding("acme", SubjectKind::Domain, "*.acme.com", &["viewer"]), false)]
#[case::adversarial_inner_whitespace(binding("acme", SubjectKind::Email, "alice @acme.com", &["viewer"]), false)]
#[case::adversarial_control_char(binding("acme", SubjectKind::MtlsSan, "spiffe://a\u{0}b", &["viewer"]), false)]
#[case::adversarial_sub_issuer_traversal(binding("acme", SubjectKind::Sub, "../x/1234", &["viewer"]), false)]
#[case::adversarial_role_traversal(binding("acme", SubjectKind::Email, "alice@acme.com", &["../operator"]), false)]
#[case::adversarial_tenant_traversal(binding("..", SubjectKind::Email, "alice@acme.com", &["viewer"]), false)]
#[case::adversarial_id_separator(RoleBinding { id: "a/b".into(), ..binding("acme", SubjectKind::Email, "alice@acme.com", &["viewer"]) }, false)]
#[case::boundary_subject_at_cap(binding("acme", SubjectKind::Email, &format!("{}@acme.com", "a".repeat(MAX_SUBJECT_BYTES - 9)), &["viewer"]), true)]
#[case::adversarial_subject_over_cap(binding("acme", SubjectKind::Email, &format!("{}@acme.com", "a".repeat(MAX_SUBJECT_BYTES - 8)), &["viewer"]), false)]
#[case::boundary_roles_at_cap(RoleBinding { roles: roles_n(MAX_ROLES_PER_BINDING), ..binding("acme", SubjectKind::Email, "alice@acme.com", &[]) }, true)]
#[case::adversarial_roles_over_cap(RoleBinding { roles: roles_n(MAX_ROLES_PER_BINDING + 1), ..binding("acme", SubjectKind::Email, "alice@acme.com", &[]) }, false)]
fn validate_cases(#[case] b: RoleBinding, #[case] ok: bool) {
    assert_eq!(sanitized(b).validate().is_ok(), ok);
}

#[rstest]
#[case::positive_dedup_and_sort(&[" viewer", "agent_user", "viewer "], &["agent_user", "viewer"])]
#[case::corner_single(&["viewer"], &["viewer"])]
fn sanitize_roles_cases(#[case] roles: &[&str], #[case] want: &[&str]) {
    let b = sanitized(binding("acme", SubjectKind::Email, "a@acme.com", roles));
    assert_eq!(b.roles, want);
}

#[test]
fn boundary_granted_by_truncated_on_char_boundary() {
    let mut b = binding("acme", SubjectKind::Email, "a@acme.com", &["viewer"]);
    b.granted_by = "é".repeat(400); // 800 bytes, two per char
    b.sanitize();
    assert!(b.granted_by.len() <= 512);
    assert!(b.granted_by.chars().all(|c| c == 'é'));
}

// --- matching -------------------------------------------------------------------

#[rstest]
#[case::positive_sub(binding("acme", SubjectKind::Sub, "google/42", &["viewer"]), who("acme", "user:google/42", None, false), T0, true)]
#[case::positive_verified_email(binding("acme", SubjectKind::Email, "alice@acme.com", &["viewer"]), who("acme", "user:google/42", Some("alice@acme.com"), true), T0, true)]
// desc: a Workspace domain gets its default role on a user's first sign-in.
#[case::boundary_domain_binding_applies_to_new_user(binding("acme", SubjectKind::Domain, "acme.com", &["agent_user"]), who("acme", "user:google/new", Some("new@acme.com"), true), T0, true)]
#[case::negative_unverified_email(binding("acme", SubjectKind::Email, "alice@acme.com", &["viewer"]), who("acme", "user:google/42", Some("alice@acme.com"), false), T0, false)]
#[case::negative_unverified_domain(binding("acme", SubjectKind::Domain, "acme.com", &["viewer"]), who("acme", "user:google/42", Some("alice@acme.com"), false), T0, false)]
#[case::negative_other_sub(binding("acme", SubjectKind::Sub, "google/42", &["viewer"]), who("acme", "user:google/43", None, false), T0, false)]
#[case::negative_mtls_not_matched_before_s10(binding("acme", SubjectKind::MtlsSan, "google/42", &["viewer"]), who("acme", "user:google/42", Some("a@acme.com"), true), T0, false)]
#[case::boundary_active_until_expiry(expiring(binding("acme", SubjectKind::Sub, "google/42", &["viewer"]), T0 + 1), who("acme", "user:google/42", None, false), T0, true)]
#[case::boundary_expired_at_expiry(expiring(binding("acme", SubjectKind::Sub, "google/42", &["viewer"]), T0), who("acme", "user:google/42", None, false), T0, false)]
#[case::corner_zero_expiry_never_expires(expiring(binding("acme", SubjectKind::Sub, "google/42", &["viewer"]), 0), who("acme", "user:google/42", None, false), u64::MAX, true)]
#[case::adversarial_binding_in_tenant_a_does_not_grant_in_b(binding("acme", SubjectKind::Sub, "google/42", &["org_admin"]), who("globex", "user:google/42", None, false), T0, false)]
#[case::adversarial_service_subject_not_a_user_sub(binding("acme", SubjectKind::Sub, "google/42", &["viewer"]), who("acme", "svc:google/42", None, false), T0, false)]
#[case::adversarial_domain_suffix(binding("acme", SubjectKind::Domain, "acme.com", &["viewer"]), who("acme", "user:google/42", Some("eve@evilacme.com"), true), T0, false)]
#[case::adversarial_domain_prefix(binding("acme", SubjectKind::Domain, "acme.com", &["viewer"]), who("acme", "user:google/42", Some("eve@acme.com.evil.org"), true), T0, false)]
#[case::adversarial_subdomain_not_parent(binding("acme", SubjectKind::Domain, "acme.com", &["viewer"]), who("acme", "user:google/42", Some("eve@dev.acme.com"), true), T0, false)]
#[case::adversarial_email_case_is_not_folded_at_match(binding("acme", SubjectKind::Email, "alice@acme.com", &["viewer"]), who("acme", "user:google/42", Some("alice@acme.com.evil"), true), T0, false)]
fn applies_to_cases(
    #[case] b: RoleBinding,
    #[case] w: Who<'static>,
    #[case] now: u64,
    #[case] want: bool,
) {
    assert_eq!(b.applies_to(&w, now), want);
}

// --- operator_subjects ------------------------------------------------------------

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(ToString::to_string).collect()
}

#[rstest]
#[case::positive_email(strs(&["email:root@acme.com"]), true)]
#[case::positive_sub(strs(&["sub:google/42"]), true)]
#[case::corner_empty_list(strs(&[]), true)]
#[case::corner_trimmed_and_lowercased(strs(&["  email: Root@ACME.com "]), true)]
#[case::negative_bare_email(strs(&["root@acme.com"]), false)]
#[case::negative_unknown_prefix(strs(&["domain:acme.com"]), false)]
#[case::negative_prefix_case(strs(&["EMAIL:root@acme.com"]), false)]
#[case::negative_empty_email(strs(&["email:"]), false)]
#[case::negative_sub_without_issuer(strs(&["sub:42"]), false)]
#[case::adversarial_one_bad_entry_fails_all(strs(&["email:root@acme.com", "sub:../x/1"]), false)]
#[case::boundary_at_cap((0..MAX_OPERATOR_SUBJECTS).map(|i| format!("sub:google/{i}")).collect(), true)]
#[case::adversarial_over_cap((0..=MAX_OPERATOR_SUBJECTS).map(|i| format!("sub:google/{i}")).collect(), false)]
fn operator_subjects_parse_cases(#[case] entries: Vec<String>, #[case] ok: bool) {
    assert_eq!(OperatorSubjects::parse(&entries).is_ok(), ok);
}

#[rstest]
#[case::positive_verified_email(who("acme", "user:google/42", Some("root@acme.com"), true), true)]
#[case::positive_sub(who("globex", "user:google/7", None, false), true)]
#[case::negative_unverified_email(
    who("acme", "user:google/42", Some("root@acme.com"), false),
    false
)]
#[case::negative_other(who("acme", "user:google/43", Some("bob@acme.com"), true), false)]
#[case::adversarial_service_subject(who("acme", "svc:google/7", None, false), false)]
fn operator_subjects_contains_cases(#[case] w: Who<'static>, #[case] want: bool) {
    let ops = OperatorSubjects::parse(&strs(&["email:ROOT@acme.com", "sub:google/7"])).unwrap();
    assert!(!ops.is_empty());
    assert_eq!(ops.contains(&w), want);
}

// --- resolution ---------------------------------------------------------------------

#[rstest]
#[case::positive_union_sorted(
    strs(&["viewer"]),
    vec![binding("acme", SubjectKind::Sub, "google/42", &["reviewer", "viewer"])],
    who("acme", "user:google/42", None, false),
    &[ROLE_REVIEWER, ROLE_VIEWER],
)]
#[case::positive_bootstrap_operator(
    strs(&[]),
    vec![],
    who("acme", "user:google/42", Some("root@acme.com"), true),
    &["operator"],
)]
#[case::boundary_domain_binding_applies_to_new_user(
    strs(&[]),
    vec![binding("acme", SubjectKind::Domain, "acme.com", &[ROLE_AGENT_USER])],
    who("acme", "user:google/new", Some("new@acme.com"), true),
    &[ROLE_AGENT_USER],
)]
#[case::corner_no_bindings_keeps_claim_roles(
    strs(&["viewer"]),
    vec![],
    who("acme", "user:google/42", None, false),
    &[ROLE_VIEWER],
)]
#[case::negative_expired_binding_dropped(
    strs(&[]),
    vec![expiring(binding("acme", SubjectKind::Sub, "google/42", &[ROLE_REVIEWER]), T0)],
    who("acme", "user:google/42", None, false),
    &[],
)]
#[case::negative_unverified_bootstrap_email(
    strs(&[]),
    vec![],
    who("acme", "user:google/42", Some("root@acme.com"), false),
    &[],
)]
#[case::adversarial_binding_in_tenant_a_does_not_grant_in_b(
    strs(&[]),
    vec![binding("acme", SubjectKind::Sub, "google/42", &[ROLE_ORG_ADMIN])],
    who("globex", "user:google/42", None, false),
    &[],
)]
fn resolve_roles_cases(
    #[case] claim: Vec<String>,
    #[case] bindings: Vec<RoleBinding>,
    #[case] w: Who<'static>,
    #[case] want: &[&str],
) {
    let ops = OperatorSubjects::parse(&strs(&["email:root@acme.com"])).unwrap();
    assert_eq!(resolve_roles(&claim, &bindings, &ops, &w, T0), want);
}

// --- permission management ------------------------------------------------------------

const ALICE: &str = "user:google/alice";

#[rstest]
// desc: access_admin hands out access_admin to someone else in its tenant.
#[case::positive_access_admin_grants_access_admin(&[ROLE_ACCESS_ADMIN], None, binding("acme", SubjectKind::Email, "bob@acme.com", &[ROLE_ACCESS_ADMIN]), true, Ok(()))]
#[case::positive_org_admin_grants_reviewer(&[ROLE_ORG_ADMIN], None, binding("acme", SubjectKind::Domain, "acme.com", &[ROLE_REVIEWER]), true, Ok(()))]
#[case::positive_operator_grants_anything_anywhere(&["operator"], Some("root@acme.com"), binding("globex", SubjectKind::Email, "root@acme.com", &["operator"]), true, Ok(()))]
// desc: a host-global reader may grant a read-only role in another tenant.
#[case::positive_host_global_cross_tenant(&["global_reader"], None, binding("globex", SubjectKind::Email, "bob@globex.com", &[ROLE_VIEWER]), true, Ok(()))]
// desc: removing your own binding is not self-binding.
#[case::corner_remove_own_binding(&[ROLE_ORG_ADMIN], None, binding("acme", SubjectKind::Sub, "google/alice", &[ROLE_VIEWER]), false, Ok(()))]
// desc: an unknown role in a binding being removed does not block the removal.
#[case::corner_remove_binding_with_unknown_role(&[ROLE_ORG_ADMIN], None, binding("acme", SubjectKind::Email, "bob@acme.com", &["retired_role"]), false, Ok(()))]
#[case::negative_unknown_role(&[ROLE_ORG_ADMIN], None, binding("acme", SubjectKind::Email, "bob@acme.com", &["no_such_role"]), true, Err(GrantRefusal::UnknownRole))]
#[case::negative_unknown_role_even_for_operator(&["operator"], None, binding("acme", SubjectKind::Email, "bob@acme.com", &["no_such_role"]), true, Err(GrantRefusal::UnknownRole))]
#[case::negative_access_admin_cannot_grant_agent_user(&[ROLE_ACCESS_ADMIN], None, binding("acme", SubjectKind::Email, "bob@acme.com", &[ROLE_AGENT_USER]), true, Err(GrantRefusal::Escalation))]
#[case::negative_reviewer_cannot_grant_fleet_admin(&[ROLE_REVIEWER, ROLE_ACCESS_ADMIN], None, binding("acme", SubjectKind::Email, "bob@acme.com", &[ROLE_FLEET_ADMIN]), true, Err(GrantRefusal::Escalation))]
#[case::adversarial_access_admin_cannot_grant_exec(&[ROLE_ACCESS_ADMIN], None, binding("acme", SubjectKind::Email, "bob@acme.com", &["exec_only"]), true, Err(GrantRefusal::Escalation))]
#[case::adversarial_access_admin_cannot_grant_org_admin(&[ROLE_ACCESS_ADMIN], None, binding("acme", SubjectKind::Email, "bob@acme.com", &[ROLE_ORG_ADMIN]), true, Err(GrantRefusal::Escalation))]
#[case::adversarial_org_admin_cannot_grant_operator(&[ROLE_ORG_ADMIN], None, binding("acme", SubjectKind::Email, "bob@acme.com", &["operator"]), true, Err(GrantRefusal::HostGlobal))]
#[case::adversarial_org_admin_cannot_bind_in_other_tenant(&[ROLE_ORG_ADMIN], None, binding("globex", SubjectKind::Email, "bob@globex.com", &[ROLE_VIEWER]), true, Err(GrantRefusal::HostGlobal))]
#[case::adversarial_org_admin_cannot_remove_in_other_tenant(&[ROLE_ORG_ADMIN], None, binding("globex", SubjectKind::Email, "bob@globex.com", &[ROLE_VIEWER]), false, Err(GrantRefusal::HostGlobal))]
// desc: removing a grant beyond your own power is refused too.
#[case::adversarial_access_admin_cannot_remove_org_admin(&[ROLE_ACCESS_ADMIN], None, binding("acme", SubjectKind::Email, "bob@acme.com", &[ROLE_ORG_ADMIN]), false, Err(GrantRefusal::Escalation))]
#[case::adversarial_self_binding_denied(&[ROLE_ORG_ADMIN], None, binding("acme", SubjectKind::Sub, "google/alice", &[ROLE_VIEWER]), true, Err(GrantRefusal::SelfBinding))]
#[case::adversarial_self_binding_by_email(&[ROLE_ACCESS_ADMIN], Some("alice@acme.com"), binding("acme", SubjectKind::Email, "alice@acme.com", &[ROLE_ACCESS_ADMIN]), true, Err(GrantRefusal::SelfBinding))]
#[case::adversarial_self_binding_by_domain(&[ROLE_ACCESS_ADMIN], Some("alice@acme.com"), binding("acme", SubjectKind::Domain, "acme.com", &[ROLE_ACCESS_ADMIN]), true, Err(GrantRefusal::SelfBinding))]
fn check_binding_write_cases(
    #[case] roles: &[&str],
    #[case] email: Option<&str>,
    #[case] b: RoleBinding,
    #[case] granting: bool,
    #[case] want: Result<(), GrantRefusal>,
) {
    let p = principal("acme", ALICE, roles);
    let granter = Granter {
        principal: &p,
        email,
    };
    assert_eq!(check_binding_write(&catalog(), granter, &b, granting), want);
}

fn with_id(mut b: RoleBinding, id: &str) -> RoleBinding {
    b.id = id.into();
    b
}

#[rstest]
#[case::corner_last_binding_admin_not_deletable(
    vec![binding("acme", SubjectKind::Email, "a@acme.com", &[ROLE_ACCESS_ADMIN])],
    vec![],
    true,
)]
#[case::positive_another_admin_remains(
    vec![binding("acme", SubjectKind::Email, "a@acme.com", &[ROLE_ACCESS_ADMIN]), with_id(binding("acme", SubjectKind::Email, "b@acme.com", &[ROLE_ORG_ADMIN]), "b2")],
    vec![with_id(binding("acme", SubjectKind::Email, "b@acme.com", &[ROLE_ORG_ADMIN]), "b2")],
    false,
)]
#[case::negative_downgrade_of_last_admin(
    vec![binding("acme", SubjectKind::Email, "a@acme.com", &[ROLE_ACCESS_ADMIN])],
    vec![binding("acme", SubjectKind::Email, "a@acme.com", &[ROLE_VIEWER])],
    true,
)]
#[case::corner_no_admin_before(
    vec![binding("acme", SubjectKind::Email, "a@acme.com", &[ROLE_VIEWER])],
    vec![],
    false,
)]
#[case::corner_empty(vec![], vec![], false)]
// desc: an expired admin binding was not protecting anything.
#[case::boundary_expired_admin(
    vec![expiring(binding("acme", SubjectKind::Email, "a@acme.com", &[ROLE_ACCESS_ADMIN]), T0)],
    vec![],
    false,
)]
// desc: a host-global operator binding counts as a binding admin in the tenant.
#[case::positive_operator_binding_counts(
    vec![binding("acme", SubjectKind::Email, "a@acme.com", &["operator"])],
    vec![],
    true,
)]
fn removes_last_admin_cases(
    #[case] before: Vec<RoleBinding>,
    #[case] after: Vec<RoleBinding>,
    #[case] want: bool,
) {
    assert_eq!(
        removes_last_admin(&catalog(), &before, &after, "acme", T0),
        want
    );
}

// --- store ------------------------------------------------------------------------------

fn store() -> (BindingStore, Arc<dyn Backend>) {
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    (
        BindingStore::new(backend.clone(), Arc::new(Fixed(T0))),
        backend,
    )
}

#[tokio::test]
async fn positive_store_round_trip() {
    let (s, _) = store();
    assert_eq!(s.now(), T0);
    let put = s
        .put(binding(
            "acme",
            SubjectKind::Email,
            " Bob@ACME.com",
            &["viewer", "viewer"],
        ))
        .await
        .expect("put");
    assert_eq!(put.subject, "bob@acme.com");
    assert_eq!(put.roles, ["viewer"]);
    assert_eq!(s.get("acme", "b1").await.unwrap(), Some(put.clone()));
    assert_eq!(s.list("acme").await.unwrap(), vec![put]);
    assert!(
        s.list("globex").await.unwrap().is_empty(),
        "tenant firewall"
    );
    assert_eq!(s.get("globex", "b1").await.unwrap(), None);
    assert!(s.delete("acme", "b1").await.unwrap());
    assert!(!s.delete("acme", "b1").await.unwrap());
    assert_eq!(s.get("acme", "b1").await.unwrap(), None);
}

#[rstest]
#[case::adversarial_tenant_traversal("..", "b1")]
#[case::adversarial_id_traversal("acme", "../b1")]
#[case::boundary_empty_id("acme", "")]
#[tokio::test]
async fn adversarial_unsafe_keys_read_nothing(#[case] tenant: &str, #[case] id: &str) {
    let (s, _) = store();
    s.put(binding(
        "acme",
        SubjectKind::Email,
        "bob@acme.com",
        &["viewer"],
    ))
    .await
    .unwrap();
    assert_eq!(s.get(tenant, id).await.unwrap(), None);
    if id == "b1" {
        assert!(s.list(tenant).await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn negative_invalid_binding_not_stored() {
    let (s, _) = store();
    assert!(s
        .put(binding(
            "acme",
            SubjectKind::Email,
            "not-an-email",
            &["viewer"]
        ))
        .await
        .is_err());
    assert!(s.list("acme").await.unwrap().is_empty());
}

// desc: a blob stored under another tenant's key must not be served as that tenant's.
#[tokio::test]
async fn adversarial_key_mismatch_rejected() {
    let (s, backend) = store();
    let foreign = binding(
        "globex",
        SubjectKind::Email,
        "eve@globex.com",
        &["org_admin"],
    );
    backend
        .apply(&[
            Write::EnsureTenant {
                tenant: "acme".into(),
            },
            Write::Put {
                collection: COLLECTION,
                tenant: "acme".into(),
                id: "b1".into(),
                blob: foreign.encode(),
            },
        ])
        .await
        .unwrap();
    assert!(s.get("acme", "b1").await.is_err());
}

#[tokio::test]
async fn corner_lock_is_reentrant_after_drop() {
    let (s, _) = store();
    drop(s.lock().await);
    let _held = s.lock().await;
}
