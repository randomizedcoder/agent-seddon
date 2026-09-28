//! Per-RPC authorization policy (security-hardening S7,
//! docs/design/security-hardening/03-rbac.md).
//!
//! Every served RPC has one [`Gate`], looked up by [`gate_of`] from the request
//! path. The auth layer enforces it for every call that carries a verified
//! principal, so a read, the interactive agent and served exec are gated like the
//! control-plane writes. An RPC missing from the table is denied: a new RPC is
//! unreachable under auth until it is classified here, in
//! `test/mt-audit/authz.toml` (sub-check 6 keeps the two equal), and the
//! `every_served_rpc_has_a_gate` test sees it. With no principal
//! (`[auth] mode = "none"`) nothing here applies.

use agent_core::{Action, ResourceType};

/// What an RPC requires of a verified caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// Reachable without a bearer; the auth layer exempts the path.
    Public,
    /// Any verified principal (the caller's own identity).
    Authenticated,
    /// A permission in the caller's own tenant.
    Require(Action, ResourceType),
    /// The layer requires the permission; the handler adds a check on the
    /// request itself (watching another subject's session needs `observe`).
    FieldChecked(Action, ResourceType),
}

impl Gate {
    /// The permission the layer checks, if any.
    pub fn permission(self) -> Option<(Action, ResourceType)> {
        match self {
            Gate::Require(a, r) | Gate::FieldChecked(a, r) => Some((a, r)),
            Gate::Public | Gate::Authenticated => None,
        }
    }
}

/// The gate of `method` on the bare proto `service`, or `None` for an RPC this
/// build does not know. A closed match, method by method, so every RPC is a
/// conscious decision. The agent's own seams (context, provider, tokenizer, tools,
/// repo, …) are `(use, agent)`: they are what an interactive session runs on.
pub fn gate_of(service: &str, method: &str) -> Option<Gate> {
    use Action::*;
    use Gate::*;
    use ResourceType::*;
    Some(match (service, method) {
        // `Refresh` carries its own credential (the refresh handle).
        ("AuthService", "Exchange" | "Jwks" | "Refresh" | "Issuers" | "Begin") => Public,
        ("AuthService", "WhoAmI" | "Logout" | "ListMySessions" | "RevokeMySession") => {
            Authenticated
        }
        // Sessions are access control: the same permission as role bindings. The
        // request may name another tenant, which the handler checks.
        ("AuthService", "ListSessions") => FieldChecked(Read, Binding),
        ("AuthService", "RevokeSession") => FieldChecked(Write, Binding),
        // Role bindings (S8); the handler adds the tenant check and the
        // permission-management rules.
        ("AuthService", "ListBindings" | "GetBinding") => FieldChecked(Read, Binding),
        ("AuthService", "PutBinding") => FieldChecked(Write, Binding),
        ("AuthService", "DeleteBinding") => FieldChecked(Delete, Binding),

        // The interactive agent and the seams it runs on.
        ("AgentSessionService", "Send") => Require(Use, Agent),
        ("AgentSessionService", "Subscribe" | "Snapshot") => FieldChecked(Use, Agent),
        ("SessionRegistryService", "Open" | "Close" | "Heartbeat") => Require(Use, Agent),
        (
            "SessionService",
            "Checkpoint" | "List" | "Restore" | "Branch" | "Undo" | "Fork" | "Diff" | "Prune",
        ) => Require(Use, Agent),
        ("Memory", "Recall" | "Append" | "Distill")
        | ("Episodic", "Append" | "Recent")
        | ("Semantic", "Recall" | "Distill")
        | ("DimensionService", "Summarize" | "Recall")
        | ("DigestService", "Put" | "Query") => Require(Use, Agent),
        ("SearchService", "Status" | "Capabilities" | "Reindex" | "Search" | "ListFiles") => {
            Require(Use, Agent)
        }
        ("ToolService", "DescribeAll" | "Execute") => Require(Use, Agent),
        (
            "RepoService",
            "Resolve" | "ReadFile" | "ListTree" | "Diff" | "Grep" | "Log" | "Branches" | "Status"
            | "Fetch" | "WorktreeAdd" | "WorktreeList" | "WorktreeRemove" | "CreateCheckpoint"
            | "Push",
        ) => Require(Use, Agent),
        (
            "AstService",
            "Status" | "Capabilities" | "Reindex" | "FindSymbol" | "Implementations"
            | "InterfaceOf" | "Callers" | "Callees" | "Callchain" | "BlastRadius"
            | "DependencyPath",
        ) => Require(Use, Agent),
        ("ContextService", "Assemble" | "Compact")
        | ("EmbedService", "Capabilities" | "EmbedQuery" | "EmbedDocs")
        | ("LlmPoolService", "Health" | "Complete")
        | ("LspService", "Open" | "Request" | "Capabilities" | "Shutdown")
        | ("ModeService", "Classify")
        | ("Policy", "Authorize")
        | ("Provider", "Capabilities" | "Complete" | "Stream")
        | ("ReferenceService", "Resolve")
        | ("FactCollectorService", "Collect")
        | ("ScannerService", "Scan")
        | ("TokenizerService", "Count" | "CountMessages")
        | ("WebService", "Fetch")
        | ("WebSearchService", "Search" | "Status" | "Capabilities") => Require(Use, Agent),
        (
            "ForgeService",
            "GetPr" | "ListPrs" | "ListIssues" | "ImportIssue" | "CreatePr" | "Comment"
            | "ReviewPr",
        )
        | ("TaskService", "Write" | "Update" | "List" | "Clear") => Require(Use, Agent),
        ("ProviderRegistryService", "Route") => Require(Use, Agent),

        // Served arbitrary execution.
        ("SandboxService", "Exec" | "Capabilities")
        | ("PtyService", "Open" | "Write" | "Read" | "Resize" | "Close" | "List" | "Get") => {
            Require(Use, Exec)
        }

        // Reviews: the drafts, and the roster that produces them.
        ("ReviewFleetService", "ListReviews" | "GetReview") => Require(Read, Review),
        ("ReviewFleetService", "UpdateReview") => Require(Write, Review),
        ("ReviewFleetService", "Approve") => Require(Approve, Review),
        ("ReviewFleetService", "ReviewNow") => Require(Trigger, Fleet),
        ("ReviewFleetService", "List" | "Get" | "Preflight") => Require(Read, Fleet),
        ("ReviewFleetService", "Put" | "SetEnabled") => Require(Write, Fleet),
        ("ReviewFleetService", "Delete") => Require(Delete, Fleet),

        // Repo onboarding: forge credentials and chat transports.
        ("ForgeRegistryService", "List" | "Get") => Require(Read, ForgeRegistry),
        ("ForgeRegistryService", "Put") => Require(Write, ForgeRegistry),
        ("ForgeRegistryService", "Delete") => Require(Delete, ForgeRegistry),
        ("TransportRegistryService", "List" | "Get") => Require(Read, TransportRegistry),
        ("TransportRegistryService", "Put") => Require(Write, TransportRegistry),
        ("TransportRegistryService", "Delete") => Require(Delete, TransportRegistry),

        // LLM upstreams and routing.
        ("ProviderRegistryService", "List" | "Get" | "GetPolicy" | "Health") => {
            Require(Read, Registry)
        }
        ("ProviderRegistryService", "Put" | "Enable" | "PutPolicy") => Require(Write, Registry),
        ("ProviderRegistryService", "Delete") => Require(Delete, Registry),

        // Prompts, graphs, the scheduler.
        (
            "PromptService",
            "List" | "Get" | "Select" | "PreviewAssembled" | "GetActivePersonality",
        ) => Require(Read, Prompt),
        ("PromptService", "Put" | "SetActivePersonality") => Require(Write, Prompt),
        ("PromptService", "Delete") => Require(Delete, Prompt),
        ("GraphService", "Get" | "Validate" | "DescribeNodeTypes") => Require(Read, Graph),
        ("GraphService", "Put") => Require(Write, Graph),
        ("SchedulerService", "List" | "History") => Require(Read, Scheduler),
        ("SchedulerService", "Schedule") => Require(Schedule, Scheduler),
        ("SchedulerService", "Cancel") => Require(Delete, Scheduler),

        // Observability.
        ("MetricsProxyService", "Query" | "QueryRange") => Require(Read, Telemetry),

        // Access control.
        ("RoleService", "List" | "Get") => Require(Read, Role),
        ("RoleService", "Put") => Require(Write, Role),
        ("RoleService", "Delete") => Require(Delete, Role),

        // The host: bootstrap config is operator-global.
        ("ConfigService", "GetSchema" | "GetValues" | "Validate" | "Status") => {
            Require(Read, Config)
        }
        ("ConfigService", "Put") => Require(Write, Config),

        _ => return None,
    })
}

/// Whether the RPC at `path` is sensitive enough that the caller's auth session
/// must still be live, not just the token unexpired (security-hardening S6, D10):
/// approving a review, served exec, and role / binding / config writes.
#[cfg(any(feature = "auth", test))]
pub fn is_sensitive(path: &str) -> bool {
    use Action::*;
    use ResourceType::*;
    let permission = rpc_of(path)
        .and_then(|(s, m)| gate_of(s, m))
        .and_then(Gate::permission);
    matches!(
        permission,
        Some((Approve, _) | (Use, Exec) | (Write | Delete, Role | Binding | Config))
    )
}

/// The `(service, method)` of an `agent.v1` request path, or `None` for any other
/// shape (the same parse as [`super::identity_policy::service_of`]).
pub fn rpc_of(path: &str) -> Option<(&str, &str)> {
    let service = super::identity_policy::service_of(path)?;
    let method = path.rsplit_once('/')?.1;
    Some((service, method))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn every_served_rpc_has_a_gate() {
        // The descriptor set is every RPC the build serves; a missing row would be
        // denied to every authenticated caller.
        let missing: Vec<_> = agent_proto::method_paths()
            .iter()
            .filter(|p| rpc_of(p).and_then(|(s, m)| gate_of(s, m)).is_none())
            .cloned()
            .collect();
        assert!(missing.is_empty(), "RPCs with no gate: {missing:?}");
    }

    #[rstest]
    // positive: the defining permission of each area.
    #[case::positive_send_uses_agent(
        "/agent.v1.AgentSessionService/Send",
        Some(Gate::Require(Action::Use, ResourceType::Agent))
    )]
    #[case::positive_approve_is_review(
        "/agent.v1.ReviewFleetService/Approve",
        Some(Gate::Require(Action::Approve, ResourceType::Review))
    )]
    #[case::positive_roster_read(
        "/agent.v1.ReviewFleetService/List",
        Some(Gate::Require(Action::Read, ResourceType::Fleet))
    )]
    #[case::positive_exec(
        "/agent.v1.SandboxService/Exec",
        Some(Gate::Require(Action::Use, ResourceType::Exec))
    )]
    #[case::positive_pty_is_exec(
        "/agent.v1.PtyService/Write",
        Some(Gate::Require(Action::Use, ResourceType::Exec))
    )]
    #[case::positive_config_read(
        "/agent.v1.ConfigService/GetValues",
        Some(Gate::Require(Action::Read, ResourceType::Config))
    )]
    #[case::positive_metrics_are_telemetry(
        "/agent.v1.MetricsProxyService/Query",
        Some(Gate::Require(Action::Read, ResourceType::Telemetry))
    )]
    // corner: the handler adds the ownership check.
    #[case::corner_subscribe_is_field_checked(
        "/agent.v1.AgentSessionService/Subscribe",
        Some(Gate::FieldChecked(Action::Use, ResourceType::Agent))
    )]
    #[case::corner_whoami_is_authenticated(
        "/agent.v1.AuthService/WhoAmI",
        Some(Gate::Authenticated)
    )]
    #[case::corner_exchange_is_public("/agent.v1.AuthService/Exchange", Some(Gate::Public))]
    #[case::corner_begin_is_public("/agent.v1.AuthService/Begin", Some(Gate::Public))]
    #[case::corner_issuers_is_public("/agent.v1.AuthService/Issuers", Some(Gate::Public))]
    // boundary: Route is what the agent calls; the rest of the registry is admin.
    #[case::boundary_route_uses_agent(
        "/agent.v1.ProviderRegistryService/Route",
        Some(Gate::Require(Action::Use, ResourceType::Agent))
    )]
    #[case::boundary_upstream_write(
        "/agent.v1.ProviderRegistryService/Put",
        Some(Gate::Require(Action::Write, ResourceType::Registry))
    )]
    // negative: unknown RPCs have no gate (the layer denies them).
    #[case::negative_unknown_method("/agent.v1.Memory/DropAll", None)]
    #[case::negative_unknown_service("/agent.v1.Nope/List", None)]
    #[case::negative_foreign_package("/grpc.health.v1.Health/Check", None)]
    // adversarial: path tricks name no RPC.
    #[case::adversarial_extra_segment("/agent.v1.Memory/Recall/x", None)]
    #[case::adversarial_case_folded("/agent.v1.memory/recall", None)]
    #[case::adversarial_empty_method("/agent.v1.Memory/", None)]
    #[case::adversarial_traversal("/agent.v1.../Memory/Recall", None)]
    fn gate_of_path(#[case] path: &str, #[case] want: Option<Gate>) {
        assert_eq!(
            rpc_of(path).and_then(|(s, m)| gate_of(s, m)),
            want,
            "{path}"
        );
    }

    #[rstest]
    #[case::positive_approve("/agent.v1.ReviewFleetService/Approve", true)]
    #[case::positive_exec("/agent.v1.SandboxService/Exec", true)]
    #[case::positive_role_write("/agent.v1.RoleService/Put", true)]
    #[case::positive_role_delete("/agent.v1.RoleService/Delete", true)]
    #[case::positive_config_write("/agent.v1.ConfigService/Put", true)]
    #[case::positive_revoke_session("/agent.v1.AuthService/RevokeSession", true)]
    #[case::negative_review_read("/agent.v1.ReviewFleetService/GetReview", false)]
    #[case::negative_draft_edit("/agent.v1.ReviewFleetService/UpdateReview", false)]
    #[case::negative_role_read("/agent.v1.RoleService/List", false)]
    #[case::corner_authenticated_only("/agent.v1.AuthService/Logout", false)]
    #[case::corner_public("/agent.v1.AuthService/Refresh", false)]
    #[case::adversarial_unknown_rpc("/agent.v1.RoleService/PutAll", false)]
    fn sensitive_cases(#[case] path: &str, #[case] want: bool) {
        assert_eq!(is_sensitive(path), want, "{path}");
    }
}
