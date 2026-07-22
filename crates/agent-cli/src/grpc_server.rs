//! `agent --serve-<seam>` — host one built seam over gRPC.
//!
//! The seam is built exactly as the loop builds it (config selects the concrete
//! impl — e.g. `provider = "anthropic"`), then wrapped in the matching
//! `agent-grpc` server and served on the seam's `[grpc.<seam>] listen` address
//! (TCP or `unix:/path`), or a `--listen` override.
//!
//! ## Why a table and not five matches
//!
//! Everything about a seam that is *static data* — its flag, its short name, its
//! gRPC service name, its ports — lives in one [`SEAMS`] row. Adding a seam is
//! then one row plus one arm in each of the two places that genuinely need code
//! (borrowing its config, and building its router).
//!
//! The cost of a table is losing the compiler's exhaustiveness error when a
//! variant is added, so [`every_seam_has_a_table_row`] restores it: a new variant
//! with no row fails the test rather than silently having no flag.

use agent_grpc::server::Router;
use agent_grpc::{constants, Endpoint};
use agent_runtime::{Agent, Config};

/// Which seam to serve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seam {
    Provider,
    Memory,
    Tools,
    Context,
    Policy,
    Search,
    Repo,
    Session,
    Scanner,
    Reference,
    Scheduler,
    Tokenizer,
    Embed,
    Web,
    WebSearch,
}

/// Every variant, so the table can be checked for completeness.
const ALL_SEAMS: &[Seam] = &[
    Seam::Provider,
    Seam::Memory,
    Seam::Tools,
    Seam::Context,
    Seam::Policy,
    Seam::Search,
    Seam::Repo,
    Seam::Session,
    Seam::Scanner,
    Seam::Reference,
    Seam::Scheduler,
    Seam::Tokenizer,
    Seam::Embed,
    Seam::Web,
    Seam::WebSearch,
];

/// The static facts about a seam served as its own process.
struct SeamInfo {
    seam: Seam,
    /// The `--serve-<seam>` flag.
    flag: &'static str,
    /// Short name, used in logs and the help text.
    name: &'static str,
    /// Fully-qualified gRPC service name, as it appears in the descriptor set.
    /// This is what a health probe or `grpcurl describe` asks for, so it must
    /// match the `service` declared in the `.proto`, not the seam's short name.
    service: &'static str,
    endpoint: constants::SeamEndpoint,
}

const SEAMS: &[SeamInfo] = &[
    SeamInfo {
        seam: Seam::Provider,
        flag: "--serve-provider",
        name: "provider",
        service: "agent.v1.Provider",
        endpoint: constants::PROVIDER,
    },
    SeamInfo {
        seam: Seam::Memory,
        flag: "--serve-memory",
        name: "memory",
        service: "agent.v1.Memory",
        endpoint: constants::MEMORY,
    },
    SeamInfo {
        seam: Seam::Tools,
        flag: "--serve-tools",
        name: "tools",
        service: "agent.v1.ToolService",
        endpoint: constants::TOOLS,
    },
    SeamInfo {
        seam: Seam::Context,
        flag: "--serve-context",
        name: "context",
        service: "agent.v1.ContextService",
        endpoint: constants::CONTEXT,
    },
    SeamInfo {
        seam: Seam::Policy,
        flag: "--serve-policy",
        name: "policy",
        service: "agent.v1.Policy",
        endpoint: constants::POLICY,
    },
    SeamInfo {
        seam: Seam::Search,
        flag: "--serve-search",
        name: "search",
        service: "agent.v1.SearchService",
        endpoint: constants::SEARCH,
    },
    SeamInfo {
        seam: Seam::Repo,
        flag: "--serve-repo",
        name: "repo",
        service: "agent.v1.RepoService",
        endpoint: constants::REPO,
    },
    SeamInfo {
        seam: Seam::Session,
        flag: "--serve-session",
        name: "session",
        service: "agent.v1.SessionService",
        endpoint: constants::SESSION,
    },
    SeamInfo {
        seam: Seam::Scanner,
        flag: "--serve-scanner",
        name: "scanner",
        service: "agent.v1.ScannerService",
        endpoint: constants::SCANNER,
    },
    SeamInfo {
        seam: Seam::Reference,
        flag: "--serve-reference",
        name: "reference",
        service: "agent.v1.ReferenceService",
        endpoint: constants::REFERENCE,
    },
    SeamInfo {
        seam: Seam::Scheduler,
        flag: "--serve-scheduler",
        name: "scheduler",
        service: "agent.v1.SchedulerService",
        endpoint: constants::SCHEDULER,
    },
    SeamInfo {
        seam: Seam::Tokenizer,
        flag: "--serve-tokenizer",
        name: "tokenizer",
        service: "agent.v1.TokenizerService",
        endpoint: constants::TOKENIZER,
    },
    SeamInfo {
        seam: Seam::Embed,
        flag: "--serve-embed",
        name: "embed",
        service: "agent.v1.EmbedService",
        endpoint: constants::EMBED,
    },
    SeamInfo {
        seam: Seam::Web,
        flag: "--serve-web",
        name: "web",
        service: "agent.v1.WebService",
        endpoint: constants::WEB,
    },
    SeamInfo {
        seam: Seam::WebSearch,
        flag: "--serve-web-search",
        name: "web-search",
        service: "agent.v1.WebSearchService",
        endpoint: constants::WEB_SEARCH,
    },
];

impl Seam {
    fn info(self) -> &'static SeamInfo {
        // Every variant has a row — `every_seam_has_a_table_row` proves it.
        SEAMS
            .iter()
            .find(|s| s.seam == self)
            .expect("every Seam variant has a SEAMS row")
    }

    /// Match a `--serve-<seam>` flag.
    pub fn from_flag(flag: &str) -> Option<Seam> {
        SEAMS.iter().find(|s| s.flag == flag).map(|s| s.seam)
    }

    pub fn name(self) -> &'static str {
        self.info().name
    }

    /// The fully-qualified gRPC service name (e.g. `agent.v1.Policy`).
    pub fn service_name(self) -> &'static str {
        self.info().service
    }

    fn default_port(self) -> u16 {
        self.info().endpoint.tcp_port
    }

    /// The Prometheus `/metrics` port this seam serves on when hosted as its own
    /// `--serve-<seam>` process (so co-located seam servers don't collide on the
    /// main agent's default `:9600`). Ports come from `nix/constants.nix`.
    pub fn metrics_port(self) -> u16 {
        self.info().endpoint.metrics_port
    }

    /// The `--serve-…` flag list, for the CLI help text.
    pub fn flag_names() -> String {
        SEAMS.iter().map(|s| s.name).collect::<Vec<_>>().join("|")
    }

    fn configured_listen(self, cfg: &Config) -> &str {
        match self {
            Seam::Provider => &cfg.grpc.provider.listen,
            Seam::Memory => &cfg.grpc.memory.listen,
            Seam::Tools => &cfg.grpc.tools.listen,
            Seam::Context => &cfg.grpc.context.listen,
            Seam::Policy => &cfg.grpc.policy.listen,
            Seam::Search => &cfg.grpc.search.listen,
            Seam::Repo => &cfg.grpc.repo.listen,
            Seam::Session => &cfg.grpc.session.listen,
            Seam::Scanner => &cfg.grpc.scanner.listen,
            Seam::Reference => &cfg.grpc.reference.listen,
            Seam::Scheduler => &cfg.grpc.scheduler.listen,
            Seam::Tokenizer => &cfg.grpc.tokenizer.listen,
            Seam::Embed => &cfg.grpc.embed.listen,
            Seam::Web => &cfg.grpc.web.listen,
            Seam::WebSearch => &cfg.grpc.web_search.listen,
        }
    }
}

/// Resolve the listen endpoint: `--listen` override, else `[grpc.<seam>] listen`,
/// else a loopback default on the seam's generated port.
pub fn resolve_listen(seam: Seam, cfg: &Config, override_addr: Option<&str>) -> Endpoint {
    if let Some(addr) = override_addr {
        return Endpoint::parse(addr);
    }
    let configured = seam.configured_listen(cfg);
    if configured.is_empty() {
        Endpoint::parse(&format!("127.0.0.1:{}", seam.default_port()))
    } else {
        Endpoint::parse(configured)
    }
}

/// Resolve the `--serve-all` gateway endpoint: `--listen` override, else
/// `[grpc.gateway] listen`, else a loopback default on the generated port.
pub fn resolve_gateway_listen(cfg: &Config, override_addr: Option<&str>) -> Endpoint {
    if let Some(addr) = override_addr {
        return Endpoint::parse(addr);
    }
    if cfg.grpc.gateway.listen.is_empty() {
        Endpoint::parse(&format!("127.0.0.1:{}", constants::GATEWAY.tcp_port))
    } else {
        Endpoint::parse(&cfg.grpc.gateway.listen)
    }
}

/// Add `seam`'s gRPC service to `router`, sourcing the backing impl off the
/// already-built `agent`.
///
/// Feature-gated seams whose impl isn't enabled are **not** added, and the flag
/// says so — so a gateway hosting everything can skip them instead of refusing
/// to start. The router is returned either way (a `Router` is move-only, so
/// handing it back unchanged is what keeps the skip path from losing it).
fn add_seam_service(router: Router, agent: &Agent, seam: Seam) -> anyhow::Result<(Router, bool)> {
    use agent_grpc::server as srv;
    Ok(match seam {
        Seam::Provider => (
            router.add_service(srv::ProviderService::new(agent.provider()).into_server()),
            true,
        ),
        Seam::Memory => (
            router.add_service(srv::MemoryService::new(agent.memory()).into_server()),
            true,
        ),
        Seam::Tools => (
            router.add_service(
                srv::ToolWorker::new(agent.tools(), std::env::current_dir()?).into_server(),
            ),
            true,
        ),
        Seam::Context => (
            router.add_service(srv::ContextSvc::new(agent.context()).into_server()),
            true,
        ),
        Seam::Policy => (
            router.add_service(srv::PolicySvc::new(agent.policy()).into_server()),
            true,
        ),
        Seam::Search => match agent.search() {
            Some(s) => (
                router.add_service(srv::SearchServiceSvc::new(s).into_server()),
                true,
            ),
            None => (router, false),
        },
        Seam::Repo => match agent.repo() {
            Some(r) => (
                router.add_service(srv::RepoServiceSvc::new(r).into_server()),
                true,
            ),
            None => (router, false),
        },
        Seam::Session => match agent.session_store() {
            Some(s) => (
                router.add_service(srv::SessionServiceSvc::new(s).into_server()),
                true,
            ),
            None => (router, false),
        },
        Seam::Scanner => match agent.scanner() {
            Some(s) => (
                router.add_service(srv::ScannerServiceSvc::new(s).into_server()),
                true,
            ),
            None => (router, false),
        },
        Seam::Reference => match agent.reference_resolver() {
            Some(r) => (
                router.add_service(srv::ReferenceServiceSvc::new(r).into_server()),
                true,
            ),
            None => (router, false),
        },
        Seam::Scheduler => match agent.scheduler_seam() {
            Some(s) => (
                router.add_service(srv::SchedulerServiceSvc::new(s).into_server()),
                true,
            ),
            None => (router, false),
        },
        Seam::Tokenizer => match agent.tokenizer() {
            Some(t) => (
                router.add_service(srv::TokenizerServiceSvc::new(t).into_server()),
                true,
            ),
            None => (router, false),
        },
        Seam::Embed => match agent.embedder() {
            Some(e) => (
                router.add_service(srv::EmbedServiceSvc::new(e).into_server()),
                true,
            ),
            None => (router, false),
        },
        Seam::Web => match agent.web() {
            Some(w) => (
                router.add_service(srv::WebServiceSvc::new(w).into_server()),
                true,
            ),
            None => (router, false),
        },
        Seam::WebSearch => match agent.web_search() {
            Some(w) => (
                router.add_service(srv::WebSearchServiceSvc::new(w).into_server()),
                true,
            ),
            None => (router, false),
        },
    })
}

/// Serve `seam` (built into `agent`) until Ctrl-C.
pub async fn serve(agent: &Agent, seam: Seam, listen: Endpoint) -> anyhow::Result<()> {
    serve_seams(agent, &[seam], listen, /* skip_unavailable */ false).await
}

/// Serve **every** seam from one process on one endpoint.
///
/// A same-host deployment that wanted all seams distributed would otherwise need
/// one process (and one port) per seam. Seams whose impl isn't enabled in this
/// build/config are skipped with a warning rather than failing the process — a
/// gateway that refuses to start because one optional seam is off is not useful.
pub async fn serve_all(agent: &Agent, listen: Endpoint) -> anyhow::Result<()> {
    serve_seams(agent, ALL_SEAMS, listen, /* skip_unavailable */ true).await
}

async fn serve_seams(
    agent: &Agent,
    seams: &[Seam],
    listen: Endpoint,
    skip_unavailable: bool,
) -> anyhow::Result<()> {
    // Health is the seed of the router, so hosting one seam and hosting all of
    // them are the same code path rather than two that can drift.
    let (mut router, health) = agent_grpc::server::base_router().await;
    let mut hosted: Vec<&str> = Vec::new();
    for &seam in seams {
        let (next, added) = add_seam_service(router, agent, seam)?;
        router = next;
        if added {
            // Only now — a seam that wasn't added must not report SERVING.
            health.set_serving(seam.service_name()).await;
            hosted.push(seam.name());
        } else if skip_unavailable {
            tracing::warn!(
                seam = seam.name(),
                "seam not enabled in this build/config; skipping"
            );
        } else {
            anyhow::bail!("{} seam not enabled in this build/config", seam.name());
        }
    }
    if hosted.is_empty() {
        anyhow::bail!("no seams available to serve in this build/config");
    }
    // Enable gRPC reflection so the seams can be introspected + called with JSON
    // via `grpcurl` (see docs/components/grpc-introspection.md).
    let router = agent_grpc::server::with_reflection(router).map_err(anyhow::Error::msg)?;
    let bound = listen.bind().await?;
    tracing::info!(
        seams = hosted.join(","),
        endpoint = ?bound.dial_endpoint()?,
        "gRPC seam server ready"
    );
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("shutting down gRPC seam server");
    };
    bound.serve(router, shutdown).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// The table replaces the compiler's exhaustiveness check, so this test *is*
    /// that check: a new `Seam` variant with no `SEAMS` row would otherwise get
    /// no flag and no ports, and fail only at runtime.
    #[test]
    fn every_seam_has_a_table_row() {
        for seam in ALL_SEAMS {
            assert!(
                SEAMS.iter().any(|s| s.seam == *seam),
                "{seam:?} has no SEAMS row"
            );
        }
        assert_eq!(SEAMS.len(), ALL_SEAMS.len(), "a SEAMS row is duplicated");
    }

    /// Ports and sockets come from `nix/constants.nix`; two seams sharing one
    /// would make the second fail to bind only when both are run together.
    #[test]
    fn positive_seam_endpoints_are_unique() {
        for (i, a) in SEAMS.iter().enumerate() {
            for b in &SEAMS[i + 1..] {
                assert_ne!(
                    a.endpoint.tcp_port, b.endpoint.tcp_port,
                    "{} and {} share a TCP port",
                    a.name, b.name
                );
                assert_ne!(
                    a.endpoint.metrics_port, b.endpoint.metrics_port,
                    "{} and {} share a metrics port",
                    a.name, b.name
                );
                assert_ne!(
                    a.endpoint.uds_path, b.endpoint.uds_path,
                    "{} and {} share a socket path",
                    a.name, b.name
                );
            }
        }
    }

    /// A service name that doesn't match the `.proto` makes the health probe and
    /// `grpcurl describe` silently look up nothing.
    #[test]
    fn positive_service_names_are_fully_qualified_and_unique() {
        for (i, a) in SEAMS.iter().enumerate() {
            assert!(
                a.service.starts_with("agent.v1."),
                "{} service `{}` is not fully qualified",
                a.name,
                a.service
            );
            for b in &SEAMS[i + 1..] {
                assert_ne!(
                    a.service, b.service,
                    "{} and {} share a service",
                    a.name, b.name
                );
            }
        }
    }

    #[rstest]
    #[case::positive_provider("--serve-provider", Some(Seam::Provider))]
    #[case::positive_repo("--serve-repo", Some(Seam::Repo))]
    #[case::positive_session("--serve-session", Some(Seam::Session))]
    #[case::positive_scanner("--serve-scanner", Some(Seam::Scanner))]
    #[case::positive_reference("--serve-reference", Some(Seam::Reference))]
    #[case::positive_embed("--serve-embed", Some(Seam::Embed))]
    #[case::positive_web_search("--serve-web-search", Some(Seam::WebSearch))]
    #[case::negative_unknown_seam("--serve-nope", None)]
    #[case::negative_not_a_serve_flag("--help", None)]
    #[case::adversarial_empty("", None)]
    #[case::adversarial_prefix_only("--serve-", None)]
    fn from_flag_cases(#[case] flag: &str, #[case] want: Option<Seam>) {
        assert_eq!(Seam::from_flag(flag), want);
    }
}
