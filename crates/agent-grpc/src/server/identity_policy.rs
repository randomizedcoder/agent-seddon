//! Per-service identity policy (security-hardening S2,
//! docs/design/security-hardening/05-identity-and-tenancy.md).
//!
//! Every served gRPC service belongs to one [`IdentityClass`], the same closed
//! vocabulary as the mt-audit manifest (`test/mt-audit/manifest.toml`), which
//! sub-check 5 (`identity-policy`) keeps equal to [`class_of`]. When identity is
//! enforced — a verified principal is present, or `[auth] require_identity` is on
//! for the listener — [`admit`] rejects a call to a tenant-keyed service that does
//! not name a session, so it can no longer run unscoped against the shared `local`
//! tenant. A service missing from [`class_of`] is rejected outright while enforcing.

use tonic::codegen::http;
use tonic::Status;

/// How a service uses the caller's identity (the mt-audit manifest classes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityClass {
    /// Every RPC routes by the ambient `(tenant, session)` identity.
    Scoped,
    /// The tenant comes from request fields or a capability key; handlers
    /// validate it themselves (`SessionRegistry.Open` is the portal's bootstrap).
    FieldScoped,
    /// No per-tenant state; the tenant only attributes spans.
    Stateless,
    /// Deliberately process-global, gated by RBAC.
    OperatorGlobal,
    /// Stateful but one shared store (a documented non-isolation).
    SingleStore,
}

impl IdentityClass {
    /// Whether a call must name a session when identity is enforced.
    pub fn needs_session(self) -> bool {
        matches!(self, Self::Scoped | Self::SingleStore)
    }
}

/// The class of a bare proto service name (`Memory`, `SearchService`, …), or
/// `None` for a service this build does not know. A closed match: adding a
/// service without classifying it here fails the `every_served_service_has_a_class`
/// test and the mt-audit gate.
pub fn class_of(service: &str) -> Option<IdentityClass> {
    use IdentityClass::*;
    Some(match service {
        "ConfigService"
        | "DigestService"
        | "DimensionService"
        | "ForgeRegistryService"
        | "GraphService"
        | "Memory"
        | "PromptService"
        | "ProviderRegistryService"
        | "ReviewFleetService"
        | "SchedulerService"
        | "SearchService"
        | "TransportRegistryService" => Scoped,
        "AgentSessionService" | "SessionRegistryService" | "SessionService" => FieldScoped,
        "AstService"
        | "AuthService"
        | "ContextService"
        | "EmbedService"
        | "FactCollectorService"
        | "ForgeService"
        | "LlmPoolService"
        | "LspService"
        | "MetricsProxyService"
        | "ModeService"
        | "Policy"
        | "Provider"
        | "PtyService"
        | "ReferenceService"
        | "RepoService"
        | "SandboxService"
        | "ScannerService"
        | "TaskService"
        | "TokenizerService"
        | "ToolService"
        | "WebSearchService"
        | "WebService" => Stateless,
        "RoleService" => OperatorGlobal,
        "Episodic" | "Semantic" => SingleStore,
        _ => return None,
    })
}

/// The bare service name of an `agent.v1` request path (`/agent.v1.Memory/Recall`
/// ⇒ `Memory`). Any other shape names no service.
pub fn service_of(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/agent.v1.")?;
    let (service, method) = rest.split_once('/')?;
    let well_formed = !service.is_empty()
        && !method.is_empty()
        && !method.contains('/')
        && service.bytes().all(|b| b.is_ascii_alphanumeric());
    well_formed.then_some(service)
}

/// A well-formed identity header value (present and `safe_segment`-valid).
fn has_segment(headers: &http::HeaderMap, key: &str) -> bool {
    headers
        .get(key)
        .and_then(|v| v.to_str().ok())
        .is_some_and(agent_core::safe_segment)
}

/// Why [`admit`] refused a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    /// A tenant-keyed service was called without a valid identity.
    IdentityRequired,
    /// The path names no service with an identity class.
    NoPolicy,
}

impl Rejection {
    /// The gRPC status sent to the caller.
    pub fn into_status(self) -> Status {
        match self {
            Self::IdentityRequired => Status::unauthenticated("identity required"),
            Self::NoPolicy => Status::permission_denied("no identity policy for this service"),
        }
    }
}

/// Decide whether a call may proceed while identity is enforced. `principal` is
/// whether a verified token was presented (its tenant stands in for the user
/// header, which the auth layer has already rewritten).
pub fn admit(path: &str, headers: &http::HeaderMap, principal: bool) -> Result<(), Rejection> {
    let Some(class) = service_of(path).and_then(class_of) else {
        return Err(Rejection::NoPolicy);
    };
    if !class.needs_session() {
        return Ok(());
    }
    let user = principal || has_segment(headers, agent_proto::identity::USER_ID_KEY);
    if user && has_segment(headers, agent_proto::identity::SESSION_ID_KEY) {
        Ok(())
    } else {
        Err(Rejection::IdentityRequired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::positive_scoped("Memory", Some(IdentityClass::Scoped))]
    #[case::positive_field_scoped("SessionRegistryService", Some(IdentityClass::FieldScoped))]
    #[case::positive_stateless("EmbedService", Some(IdentityClass::Stateless))]
    #[case::positive_auth_service_is_stateless("AuthService", Some(IdentityClass::Stateless))]
    #[case::positive_operator_global("RoleService", Some(IdentityClass::OperatorGlobal))]
    #[case::positive_single_store("Episodic", Some(IdentityClass::SingleStore))]
    #[case::negative_unknown("NotAService", None)]
    #[case::boundary_empty("", None)]
    #[case::adversarial_case_folded("memory", None)]
    #[case::adversarial_health_is_not_classified("grpc.health.v1.Health", None)]
    fn class_of_cases(#[case] service: &str, #[case] want: Option<IdentityClass>) {
        assert_eq!(class_of(service), want);
    }

    #[rstest]
    #[case::positive_plain("/agent.v1.Memory/Recall", Some("Memory"))]
    #[case::negative_other_package("/grpc.health.v1.Health/Check", None)]
    #[case::negative_no_method("/agent.v1.Memory/", None)]
    #[case::boundary_no_slash("/agent.v1.Memory", None)]
    #[case::corner_empty_service("/agent.v1./Recall", None)]
    #[case::adversarial_extra_segment("/agent.v1.Memory/Recall/x", None)]
    #[case::adversarial_dotted_service("/agent.v1.Memory.Evil/Recall", None)]
    #[case::adversarial_traversal("/agent.v1../Recall", None)]
    fn service_of_cases(#[case] path: &str, #[case] want: Option<&str>) {
        assert_eq!(service_of(path), want);
    }

    fn headers(user: Option<&str>, session: Option<&str>) -> http::HeaderMap {
        let mut h = http::HeaderMap::new();
        if let Some(u) = user {
            h.insert(agent_proto::identity::USER_ID_KEY, u.parse().unwrap());
        }
        if let Some(s) = session {
            h.insert(agent_proto::identity::SESSION_ID_KEY, s.parse().unwrap());
        }
        h
    }

    #[rstest]
    #[case::positive_scoped_with_both("/agent.v1.Memory/Recall", Some("acme"), Some("s1"), false, Ok(()))]
    #[case::positive_token_without_session_hits_stateless("/agent.v1.EmbedService/Embed", None, None, true, Ok(()))]
    #[case::positive_principal_with_session("/agent.v1.Memory/Recall", Some("acme"), Some("s1"), true, Ok(()))]
    #[case::negative_token_without_session_on_scoped_is_unauthenticated(
        "/agent.v1.Memory/Recall",
        Some("acme"),
        None,
        true,
        Err(tonic::Code::Unauthenticated)
    )]
    #[case::negative_header_user_without_session(
        "/agent.v1.PromptService/List",
        Some("acme"),
        None,
        false,
        Err(tonic::Code::Unauthenticated)
    )]
    #[case::negative_single_store_needs_session(
        "/agent.v1.Episodic/Append",
        None,
        None,
        true,
        Err(tonic::Code::Unauthenticated)
    )]
    #[case::corner_field_scoped_open_without_session_ok("/agent.v1.SessionRegistryService/Open", None, None, true, Ok(()))]
    #[case::corner_operator_global_tenant_only("/agent.v1.RoleService/List", None, None, true, Ok(()))]
    #[case::boundary_session_without_user_no_principal(
        "/agent.v1.Memory/Recall",
        None,
        Some("s1"),
        false,
        Err(tonic::Code::Unauthenticated)
    )]
    #[case::adversarial_unknown_service_rejected(
        "/agent.v1.Shadow/Dump",
        Some("acme"),
        Some("s1"),
        true,
        Err(tonic::Code::PermissionDenied)
    )]
    #[case::adversarial_traversal_session(
        "/agent.v1.Memory/Recall",
        Some("acme"),
        Some(".."),
        true,
        Err(tonic::Code::Unauthenticated)
    )]
    #[case::adversarial_separator_user(
        "/agent.v1.Memory/Recall",
        Some("a/b"),
        Some("s1"),
        false,
        Err(tonic::Code::Unauthenticated)
    )]
    #[case::adversarial_malformed_path(
        "/agent.v1.Memory/Recall/extra",
        Some("acme"),
        Some("s1"),
        true,
        Err(tonic::Code::PermissionDenied)
    )]
    fn admit_cases(
        #[case] path: &str,
        #[case] user: Option<&str>,
        #[case] session: Option<&str>,
        #[case] principal: bool,
        #[case] want: Result<(), tonic::Code>,
    ) {
        let got =
            admit(path, &headers(user, session), principal).map_err(|r| r.into_status().code());
        assert_eq!(got, want);
    }

    /// Every service in the wire descriptor set has a class, so a new service
    /// cannot ship without an identity policy (it would be rejected while
    /// enforcing, and this test names it first).
    #[test]
    fn positive_every_served_service_has_a_class() {
        let missing: Vec<String> = agent_proto::method_paths()
            .iter()
            .filter_map(|p| service_of(p).map(str::to_string))
            .filter(|s| class_of(s).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "services without an identity class: {missing:?}"
        );
    }
}
