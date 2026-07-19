//! `agent --serve-<seam>` — host one built seam over gRPC.
//!
//! The seam is built exactly as the loop builds it (config selects the concrete
//! impl — e.g. `provider = "anthropic"`), then wrapped in the matching
//! `agent-grpc` server and served on the seam's `[grpc.<seam>] listen` address
//! (TCP or `unix:/path`), or a `--listen` override.

use agent_grpc::{constants, Endpoint};
use agent_runtime::{Agent, Config};

/// Which seam to serve.
#[derive(Clone, Copy, Debug)]
pub enum Seam {
    Provider,
    Memory,
    Tools,
    Context,
    Policy,
    Search,
    Repo,
}

impl Seam {
    /// Match a `--serve-<seam>` flag.
    pub fn from_flag(flag: &str) -> Option<Seam> {
        match flag {
            "--serve-provider" => Some(Seam::Provider),
            "--serve-memory" => Some(Seam::Memory),
            "--serve-tools" => Some(Seam::Tools),
            "--serve-context" => Some(Seam::Context),
            "--serve-policy" => Some(Seam::Policy),
            "--serve-search" => Some(Seam::Search),
            "--serve-repo" => Some(Seam::Repo),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Seam::Provider => "provider",
            Seam::Memory => "memory",
            Seam::Tools => "tools",
            Seam::Context => "context",
            Seam::Policy => "policy",
            Seam::Search => "search",
            Seam::Repo => "repo",
        }
    }

    fn default_port(self) -> u16 {
        match self {
            Seam::Provider => constants::PROVIDER.tcp_port,
            Seam::Memory => constants::MEMORY.tcp_port,
            Seam::Tools => constants::TOOLS.tcp_port,
            Seam::Context => constants::CONTEXT.tcp_port,
            Seam::Policy => constants::POLICY.tcp_port,
            Seam::Search => constants::SEARCH.tcp_port,
            Seam::Repo => constants::REPO.tcp_port,
        }
    }

    /// The Prometheus `/metrics` port this seam serves on when hosted as its own
    /// `--serve-<seam>` process (so co-located seam servers don't collide on the
    /// main agent's default `:9600`). Ports come from `nix/constants.nix`.
    pub fn metrics_port(self) -> u16 {
        match self {
            Seam::Provider => constants::PROVIDER.metrics_port,
            Seam::Memory => constants::MEMORY.metrics_port,
            Seam::Tools => constants::TOOLS.metrics_port,
            Seam::Context => constants::CONTEXT.metrics_port,
            Seam::Policy => constants::POLICY.metrics_port,
            Seam::Search => constants::SEARCH.metrics_port,
            Seam::Repo => constants::REPO.metrics_port,
        }
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

/// Serve `seam` (built into `agent`) until Ctrl-C.
pub async fn serve(agent: &Agent, seam: Seam, listen: Endpoint) -> anyhow::Result<()> {
    let router = match seam {
        Seam::Provider => agent_grpc::server::provider_router(agent.provider()),
        Seam::Memory => agent_grpc::server::memory_router(agent.memory()),
        Seam::Tools => agent_grpc::server::tools_router(agent.tools(), std::env::current_dir()?),
        Seam::Context => agent_grpc::server::context_router(agent.context()),
        Seam::Policy => agent_grpc::server::policy_router(agent.policy()),
        Seam::Search => agent_grpc::server::search_router(
            agent
                .search()
                .ok_or_else(|| anyhow::anyhow!("search seam not enabled in this build/config"))?,
        ),
        Seam::Repo => agent_grpc::server::repo_router(
            agent
                .repo()
                .ok_or_else(|| anyhow::anyhow!("git seam not enabled in this build/config"))?,
        ),
    };
    let bound = listen.bind().await?;
    tracing::info!(seam = seam.name(), endpoint = ?bound.dial_endpoint()?, "gRPC seam server ready");
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("shutting down gRPC seam server");
    };
    bound.serve(router, shutdown).await?;
    Ok(())
}
