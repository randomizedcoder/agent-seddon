//! Per-row fleet review contexts (review-fleet **multi-repo grounding**).
//!
//! A single `--serve-fleet` process hosts a roster of many repos, but grounding used to be
//! wired **once** from the process-global `[forge]` + a single `RepoBackend` rooted at the
//! process cwd — so only the one repo `[git]`/`[forge]` pointed at could actually be reviewed.
//! [`FleetReviewCtxFactory`] lifts that limit: given a roster row it resolves/creates *that
//! row's own* bare mirror under the fleet root and builds a [`ReviewOrchestrator`] bound to that
//! repo **and the row's forge** (via the existing [`crate::registry::build_session_forge`]),
//! wrapped as an [`agent_core::ReviewGrounder`]. The fleet orchestrator then fetches, worktrees,
//! and grounds each PR against the correct repo.
//!
//! **Untrusted rows.** `row.user`/`row.id` become path segments and are re-validated through
//! [`SessionKey::parse`] + [`SessionKey::path_under`] (`safe_segment`, fail-closed) before any
//! directory is created, so a hostile id cannot escape the fleet root. `row.repo`/`backend`/
//! `base_url` are validated in [`clone_url`] (each slug segment re-checked with `safe_segment`)
//! before they reach a `git clone` argument.
//!
//! Compiled only with the `review` feature (the engine + fact renderer live in `agent-review`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_core::{safe_segment, Error, FleetReviewCtx, FleetReviewFactory, FleetSession, Result};
use agent_metrics::Metrics;
use async_trait::async_trait;

use crate::agent::EngineGrounder;
use crate::config::ReviewCfg;
use crate::registry::build_session_forge;

/// Derive the `git clone` URL for a roster row from its `repo` slug (`owner__name`, or
/// gitlab subgroups `group__sub__name`), `backend`, and optional API `base_url`.
///
/// Defense-in-depth: every path segment is re-checked with [`safe_segment`] (the slug was
/// already validated at roster admission, but this is the point where it becomes a git
/// argument, so it is re-validated here) — a `..`, separator, or leading `-` is rejected, never
/// sanitized. The clone host defaults to the forge's public host; an explicit `base_url`
/// (e.g. a GitHub Enterprise / self-hosted GitLab API root) supplies the host instead, with the
/// public-GitHub API host (`api.github.com`) mapped back to its clone host (`github.com`).
fn clone_url(repo: &str, backend: &str, base_url: &str) -> std::result::Result<String, String> {
    match backend {
        "github" => {
            let (owner, name) = repo
                .split_once("__")
                .filter(|(o, n)| !o.is_empty() && !n.is_empty())
                .ok_or_else(|| format!("github repo must be `owner__name`, got `{repo}`"))?;
            if !safe_segment(owner) || !safe_segment(name) {
                return Err(format!("unsafe repo slug segment in `{repo}`"));
            }
            let host = host_root(base_url, "https://github.com", true)?;
            Ok(format!("{host}/{owner}/{name}.git"))
        }
        "gitlab" => {
            // GitLab allows nested subgroups: `group__sub__proj` → `group/sub/proj`.
            let path = repo.replace("__", "/");
            let segs: Vec<&str> = path.split('/').collect();
            if segs.iter().any(|s| s.is_empty()) {
                return Err(format!("gitlab repo has an empty path segment: `{repo}`"));
            }
            if !segs.iter().all(|s| safe_segment(s)) {
                return Err(format!("unsafe repo slug segment in `{repo}`"));
            }
            let host = host_root(base_url, "https://gitlab.com", false)?;
            Ok(format!("{host}/{path}.git"))
        }
        "gitea" => {
            // Gitea repos are `owner/name`, like GitHub; its API host is also its
            // clone host (no `api.` → bare-host remap), so `map_github_api` is false.
            let (owner, name) = repo
                .split_once("__")
                .filter(|(o, n)| !o.is_empty() && !n.is_empty())
                .ok_or_else(|| format!("gitea repo must be `owner__name`, got `{repo}`"))?;
            if !safe_segment(owner) || !safe_segment(name) {
                return Err(format!("unsafe repo slug segment in `{repo}`"));
            }
            let host = host_root(base_url, "https://gitea.com", false)?;
            Ok(format!("{host}/{owner}/{name}.git"))
        }
        "bitbucket" => {
            // Bitbucket Cloud clones `workspace/repo` from bitbucket.org; the API
            // host (api.bitbucket.org) maps back to the web/clone host.
            let (workspace, name) = repo
                .split_once("__")
                .filter(|(o, n)| !o.is_empty() && !n.is_empty())
                .ok_or_else(|| format!("bitbucket repo must be `workspace__slug`, got `{repo}`"))?;
            if !safe_segment(workspace) || !safe_segment(name) {
                return Err(format!("unsafe repo slug segment in `{repo}`"));
            }
            let host = if base_url.is_empty() {
                "https://bitbucket.org".to_string()
            } else {
                host_root(base_url, "https://bitbucket.org", false)?
                    .replace("api.bitbucket.org", "bitbucket.org")
            };
            Ok(format!("{host}/{workspace}/{name}.git"))
        }
        other => Err(format!(
            "no clone-URL rule for forge backend `{other}` (expected github | gitlab | gitea | bitbucket)"
        )),
    }
}

/// The `scheme://host[:port]` clone root: `default` when `base_url` is empty, else the
/// scheme+host parsed out of `base_url` (its API path is dropped). For GitHub, the public API
/// host `api.github.com` is mapped back to the clone host `github.com` (`map_github_api`).
fn host_root(
    base_url: &str,
    default: &str,
    map_github_api: bool,
) -> std::result::Result<String, String> {
    if base_url.is_empty() {
        return Ok(default.to_string());
    }
    let (scheme, rest) = base_url
        .split_once("://")
        .ok_or_else(|| format!("base_url `{base_url}` is not a URL"))?;
    let host = rest.split('/').next().unwrap_or("");
    if scheme.is_empty() || host.is_empty() {
        return Err(format!("base_url `{base_url}` has no host"));
    }
    if map_github_api && host == "api.github.com" {
        return Ok("https://github.com".to_string());
    }
    Ok(format!("{scheme}://{host}"))
}

/// The remote PR-ref template for a backend: the operator `[git] pr_ref_template` override
/// when set, else the forge's default (`refs/pull/{n}/head` for GitHub,
/// `refs/merge-requests/{n}/head` for GitLab). An unknown backend with no override is a config
/// error — the ref layout is never guessed.
fn pr_ref_template_for(
    backend: &str,
    override_template: &str,
) -> std::result::Result<String, String> {
    if !override_template.is_empty() {
        return Ok(override_template.to_string());
    }
    match backend {
        "github" => Ok("refs/pull/{n}/head".to_string()),
        "gitlab" => Ok("refs/merge-requests/{n}/head".to_string()),
        // Gitea mirrors GitHub's PR-ref layout.
        "gitea" => Ok("refs/pull/{n}/head".to_string()),
        // Bitbucket's own PR-ref dialect (Server/DC layout; a Cloud repo without a
        // stable PR ref can override via [git] pr_ref_template).
        "bitbucket" => Ok("refs/pull-requests/{n}/from".to_string()),
        other => Err(format!(
            "no default PR-ref template for forge backend `{other}` (set [git] pr_ref_template)"
        )),
    }
}

/// Builds a per-row [`FleetReviewCtx`] so one fleet process grounds reviews for many repos.
///
/// Each row gets an isolated workspace under `<fleet_root>/<user>/<id>/` (a bare `mirror/`
/// object store + `worktrees/`), a [`agent_git::CliBackend`] rooted at the mirror (its remote =
/// the row's clone URL, so the first `fetch_pr` bootstraps the mirror), the row's forge, and a
/// [`ReviewOrchestrator`] with the
/// **same** `[review]` collector set as the in-loop path — only the repo/forge/root differ.
/// Built contexts are cached by `row.id` so repeated triggers reuse the clone + engine.
pub(crate) struct FleetReviewCtxFactory {
    fleet_root: PathBuf,
    review: ReviewCfg,
    /// The `[git] pr_ref_template` override (empty ⇒ per-backend default).
    pr_ref_override: String,
    sandbox: Option<Arc<dyn agent_core::Sandbox>>,
    pool: Option<Arc<dyn agent_core::LlmPool>>,
    search: Option<Arc<dyn agent_core::SearchBackend>>,
    /// Byte budget for the rendered grounded brief (same knob as the in-loop review).
    budget: usize,
    metrics: Metrics,
    /// Built `(repo, grounder)` by `row.id`. A repeated trigger reuses the checkout + engine.
    cache: Mutex<HashMap<String, FleetReviewCtx>>,
}

impl FleetReviewCtxFactory {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        fleet_root: PathBuf,
        review: ReviewCfg,
        pr_ref_override: String,
        sandbox: Option<Arc<dyn agent_core::Sandbox>>,
        pool: Option<Arc<dyn agent_core::LlmPool>>,
        search: Option<Arc<dyn agent_core::SearchBackend>>,
        budget: usize,
        metrics: Metrics,
    ) -> Self {
        Self {
            fleet_root,
            review,
            pr_ref_override,
            sandbox,
            pool,
            search,
            budget,
            metrics,
            cache: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl FleetReviewFactory for FleetReviewCtxFactory {
    async fn build(&self, row: &FleetSession) -> Result<FleetReviewCtx> {
        // Cache hit: a repeated trigger for the same row reuses the checkout + engine.
        if let Some(ctx) = self
            .cache
            .lock()
            .expect("fleet review cache poisoned")
            .get(&row.id)
            .cloned()
        {
            return Ok(ctx);
        }

        // Confine the row's workspace under the fleet root: `<root>/<user>/<id>` with both
        // segments `safe_segment`-validated (fail-closed — a hostile id cannot escape).
        let base = agent_core::SessionKey::parse(&row.user, &row.id)
            .and_then(|k| k.path_under(&self.fleet_root))
            .map_err(|e| Error::Config(format!("fleet row `{}`: {e}", row.id)))?;
        // The bare mirror is the whole object store — there is no working checkout, so
        // it is BOTH the CliBackend `root` (where `resolve` runs `git rev-parse`) and the
        // `mirror` (the shared object DB). The first `fetch_pr` bootstraps it via
        // `git clone --mirror` from the row's clone URL; worktrees are checked out under
        // `worktrees/`. Rooting at an empty `repo/` dir instead would make `resolve` (and
        // thus `worktree_add`) fail with "not a git repository".
        let mirror = base.join("mirror");
        let worktrees = base.join("worktrees");
        for dir in [&mirror, &worktrees] {
            std::fs::create_dir_all(dir).map_err(|e| {
                Error::Repo(format!(
                    "creating fleet workspace `{}` failed: {e}",
                    dir.display()
                ))
            })?;
        }

        let url = clone_url(&row.repo, &row.backend, &row.base_url)
            .map_err(|e| Error::Config(format!("fleet row `{}`: {e}", row.id)))?;
        let template = pr_ref_template_for(&row.backend, &self.pr_ref_override)
            .map_err(|e| Error::Config(format!("fleet row `{}`: {e}", row.id)))?;

        let mut cli = agent_git::CliBackend::new(mirror.clone(), mirror.clone(), worktrees, url)
            .with_pr_ref_template(template);
        if let Some(sandbox) = &self.sandbox {
            cli = cli.with_sandbox(sandbox.clone());
        }
        let repo: Arc<dyn agent_core::RepoBackend> = Arc::new(cli);

        // The row's own forge (fail-closed on an unresolvable/malformed credential).
        let forge = build_session_forge(row).map_err(|e| {
            Error::Fleet(format!("fleet row `{}`: forge build failed: {e}", row.id))
        })?;

        // The engine + grounder, bound to *this* repo + forge, with the same `[review]`
        // collector set the in-loop review uses. `review_root` is the mirror (the repo
        // root the fact renderer hashes/labels; the real objects live there).
        let orch = crate::builder::build_review_orchestrator(
            mirror,
            repo.clone(),
            self.search.clone(),
            forge,
            self.sandbox.clone(),
            self.pool.clone(),
            &self.review,
            self.metrics.clone(),
        );
        let grounder: Arc<dyn agent_core::ReviewGrounder> = Arc::new(EngineGrounder {
            engine: Arc::new(orch),
            budget: self.budget,
        });

        let ctx = FleetReviewCtx { repo, grounder };
        self.cache
            .lock()
            .expect("fleet review cache poisoned")
            .insert(row.id.clone(), ctx.clone());
        Ok(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn row(id: &str, user: &str, repo: &str, backend: &str, token_ref: &str) -> FleetSession {
        FleetSession {
            id: id.into(),
            user: user.into(),
            repo: repo.into(),
            backend: backend.into(),
            base_url: String::new(),
            token_ref: token_ref.into(),
            skill: String::new(),
            slack_trigger_channel: String::new(),
            slack_progress_channel: String::new(),
            poll_secs: 0,
            enabled: true,
            created_at: 0,
            updated_at: 0,
        }
    }

    // ---- clone_url: four classes + adversarial -------------------------------

    #[rstest]
    // positive: the two live forges resolve to their public clone hosts.
    #[case::positive_github_public(
        "github clone url from owner__name",
        "randomizedcoder__rtl-fun",
        "github",
        "",
        Ok("https://github.com/randomizedcoder/rtl-fun.git")
    )]
    #[case::positive_gitlab_public(
        "gitlab clone url from group__proj",
        "acme__web",
        "gitlab",
        "",
        Ok("https://gitlab.com/acme/web.git")
    )]
    // positive: an explicit API base_url supplies a self-hosted clone host.
    #[case::positive_github_enterprise_base(
        "GHE base_url drives the clone host, api path dropped",
        "org__repo",
        "github",
        "https://ghe.example.com/api/v3",
        Ok("https://ghe.example.com/org/repo.git")
    )]
    #[case::positive_gitlab_subgroups(
        "gitlab nested subgroups expand to a path",
        "group__sub__proj",
        "gitlab",
        "",
        Ok("https://gitlab.com/group/sub/proj.git")
    )]
    // positive: gitea resolves owner__name against the public instance by default.
    #[case::positive_gitea_public(
        "gitea clone url from owner__name",
        "acme__web",
        "gitea",
        "",
        Ok("https://gitea.com/acme/web.git")
    )]
    // positive: a self-hosted gitea api base drives the clone host (path dropped,
    // and the api host is NOT remapped — gitea serves clones from the same host).
    #[case::positive_gitea_self_hosted(
        "self-hosted gitea api base_url drives the clone host",
        "org__repo",
        "gitea",
        "https://gitea.example.com/api/v1",
        Ok("https://gitea.example.com/org/repo.git")
    )]
    // adversarial: a traversal owner in a gitea slug is rejected too.
    #[case::adversarial_gitea_traversal(
        "gitea `..` owner segment rejected",
        "..__repo", "gitea", "", Err(()))]
    // positive: bitbucket clones workspace/repo from bitbucket.org by default.
    #[case::positive_bitbucket_public(
        "bitbucket clone url from workspace__slug",
        "acme__web",
        "bitbucket",
        "",
        Ok("https://bitbucket.org/acme/web.git")
    )]
    // boundary: the api.bitbucket.org base host maps back to the bitbucket.org clone host.
    #[case::boundary_bitbucket_api_host_mapped(
        "bitbucket api base_url maps to the clone host",
        "acme__web",
        "bitbucket",
        "https://api.bitbucket.org/2.0",
        Ok("https://bitbucket.org/acme/web.git")
    )]
    // adversarial: a separator in a bitbucket slug is rejected.
    #[case::adversarial_bitbucket_slash(
        "bitbucket embedded `/` splits into an unsafe segment",
        "ws__r/../x", "bitbucket", "", Err(()))]
    // boundary: public api.github.com base maps back to the github.com clone host.
    #[case::boundary_github_api_host_mapped(
        "explicit public api host maps to clone host",
        "o__r",
        "github",
        "https://api.github.com",
        Ok("https://github.com/o/r.git")
    )]
    // corner: dots and dashes in the slug are preserved (valid segment chars).
    #[case::corner_dots_and_dashes_preserved(
        "dots/dashes are valid segment chars, kept verbatim",
        "my-org__my.repo-v2",
        "github",
        "",
        Ok("https://github.com/my-org/my.repo-v2.git")
    )]
    // negative: unknown backend / missing separator / bad base_url are config errors.
    #[case::negative_unknown_backend(
        "unknown forge backend has no clone rule",
        "o__r", "svn", "", Err(()))]
    #[case::negative_no_separator(
        "github slug without `__` is rejected",
        "noseparator", "github", "", Err(()))]
    #[case::negative_base_url_without_host(
        "base_url that is not a URL is rejected",
        "o__r", "github", "not-a-url", Err(()))]
    // adversarial: traversal / separator / leading-dash slugs must be rejected.
    #[case::adversarial_traversal_owner(
        "`..` owner segment rejected (no path escape)",
        "..__repo", "github", "", Err(()))]
    #[case::adversarial_slash_in_slug(
        "embedded `/` splits into an unsafe segment, rejected",
        "o__r/../x", "github", "", Err(()))]
    #[case::adversarial_leading_dash(
        "leading-dash segment (option injection) rejected",
        "-flag__repo", "github", "", Err(()))]
    #[case::adversarial_empty_name(
        "empty name segment rejected",
        "owner__", "github", "", Err(()))]
    fn clone_url_cases(
        #[case] desc: &str,
        #[case] repo: &str,
        #[case] backend: &str,
        #[case] base_url: &str,
        #[case] expect: std::result::Result<&str, ()>,
    ) {
        let got = clone_url(repo, backend, base_url);
        match expect {
            Ok(url) => assert_eq!(got.as_deref(), Ok(url), "{desc}"),
            Err(()) => assert!(got.is_err(), "{desc}: expected Err, got {got:?}"),
        }
    }

    // ---- pr_ref_template_for: four classes -----------------------------------

    #[rstest]
    #[case::positive_github_default(
        "github default PR-ref template",
        "github",
        "",
        Ok("refs/pull/{n}/head")
    )]
    #[case::positive_gitlab_default(
        "gitlab default MR-ref template",
        "gitlab",
        "",
        Ok("refs/merge-requests/{n}/head")
    )]
    #[case::positive_gitea_default(
        "gitea mirrors github's PR-ref template",
        "gitea",
        "",
        Ok("refs/pull/{n}/head")
    )]
    #[case::positive_bitbucket_default(
        "bitbucket has its own PR-ref dialect",
        "bitbucket",
        "",
        Ok("refs/pull-requests/{n}/from")
    )]
    #[case::boundary_override_wins_over_default(
        "an explicit [git] override wins for a known backend",
        "github",
        "refs/custom/{n}/head",
        Ok("refs/custom/{n}/head")
    )]
    #[case::corner_override_wins_for_unknown_backend(
        "override lets an otherwise-unknown backend resolve",
        "bitbucket",
        "refs/custom/{n}/head",
        Ok("refs/custom/{n}/head")
    )]
    #[case::negative_unknown_backend_no_override(
        "unknown backend with no override is a config error",
        "svn", "", Err(()))]
    fn pr_ref_template_cases(
        #[case] desc: &str,
        #[case] backend: &str,
        #[case] override_template: &str,
        #[case] expect: std::result::Result<&str, ()>,
    ) {
        let got = pr_ref_template_for(backend, override_template);
        match expect {
            Ok(t) => assert_eq!(got.as_deref(), Ok(t), "{desc}"),
            Err(()) => assert!(got.is_err(), "{desc}: expected Err, got {got:?}"),
        }
    }

    // ---- FleetReviewCtxFactory::build ----------------------------------------

    fn factory(root: PathBuf) -> FleetReviewCtxFactory {
        FleetReviewCtxFactory::new(
            root,
            ReviewCfg::default(),
            String::new(),
            None,
            None,
            None,
            4096,
            Metrics::new(),
        )
    }

    // positive: a valid github row builds a repo + grounder and creates the checkout.
    #[tokio::test]
    async fn positive_builds_repo_and_grounder_for_valid_row() {
        let tmp = agent_testkit::tempdir();
        let f = factory(tmp.as_path().to_path_buf());
        let r = row(
            "rtl-fun",
            "randomizedcoder",
            "randomizedcoder__rtl-fun",
            "github",
            "",
        );
        let ctx = f.build(&r).await.expect("valid row builds");
        // The confined workspace (bare mirror + worktrees) exists under
        // <root>/<user>/<id>/ (no escape). There is no working checkout — the mirror is
        // the object store the first fetch bootstraps.
        let base = tmp.as_path().join("randomizedcoder").join("rtl-fun");
        assert!(
            base.join("mirror").is_dir(),
            "mirror dir created under the fleet root"
        );
        assert!(
            base.join("worktrees").is_dir(),
            "worktrees dir created under the fleet root"
        );
        // Both seams are wired.
        let _ = ctx.repo;
        let _ = ctx.grounder;
    }

    // positive: a second build for the same row.id reuses the cached context.
    #[tokio::test]
    async fn positive_second_build_hits_cache() {
        let tmp = agent_testkit::tempdir();
        let f = factory(tmp.as_path().to_path_buf());
        let r = row(
            "rtl-fun",
            "randomizedcoder",
            "randomizedcoder__rtl-fun",
            "github",
            "",
        );
        let a = f.build(&r).await.unwrap();
        let b = f.build(&r).await.unwrap();
        assert!(
            Arc::ptr_eq(&a.repo, &b.repo),
            "same row.id reuses the cached repo (no re-clone)"
        );
        assert!(
            Arc::ptr_eq(&a.grounder, &b.grounder),
            "same row.id reuses the cached grounder"
        );
    }

    // boundary: two distinct rows get isolated checkouts (no cross-repo bleed).
    #[tokio::test]
    async fn boundary_two_rows_get_isolated_checkouts() {
        let tmp = agent_testkit::tempdir();
        let f = factory(tmp.as_path().to_path_buf());
        let a = row(
            "rtl-fun",
            "randomizedcoder",
            "randomizedcoder__rtl-fun",
            "github",
            "",
        );
        let b = row(
            "uds-rdma-proxy",
            "randomizedcoder",
            "randomizedcoder__uds-rdma-proxy",
            "github",
            "",
        );
        let ca = f.build(&a).await.unwrap();
        let cb = f.build(&b).await.unwrap();
        assert!(
            !Arc::ptr_eq(&ca.repo, &cb.repo),
            "distinct rows → distinct repos"
        );
        assert!(
            tmp.as_path()
                .join("randomizedcoder/rtl-fun/mirror")
                .is_dir()
                && tmp
                    .as_path()
                    .join("randomizedcoder/uds-rdma-proxy/mirror")
                    .is_dir(),
            "each row has its own workspace"
        );
    }

    // negative: a malformed token_ref fails the forge build (fail-closed credential).
    #[tokio::test]
    async fn negative_malformed_token_ref_errors() {
        let tmp = agent_testkit::tempdir();
        let f = factory(tmp.as_path().to_path_buf());
        // A raw (non `env:`/`file:`) token_ref is rejected by ApiKeyRef::parse.
        let r = row(
            "rtl-fun",
            "randomizedcoder",
            "acme__web",
            "github",
            "raw-secret",
        );
        assert!(
            f.build(&r).await.is_err(),
            "malformed credential fails the build"
        );
    }

    // corner: an unbuildable clone URL (missing `__`) is a config error, not a panic.
    #[tokio::test]
    async fn corner_bad_repo_slug_errors() {
        let tmp = agent_testkit::tempdir();
        let f = factory(tmp.as_path().to_path_buf());
        let r = row("rtl-fun", "randomizedcoder", "noseparator", "github", "");
        assert!(f.build(&r).await.is_err(), "bad repo slug is a build error");
    }

    // adversarial: a hostile row id / user cannot escape the fleet root.
    #[rstest]
    #[case::traversal_id("`..` row id is rejected before any dir is made", "..", "u")]
    #[case::traversal_user("`..` user is rejected", "id", "..")]
    #[case::separator_id("separator in id is rejected", "a/b", "u")]
    #[tokio::test]
    async fn adversarial_hostile_row_id_confined(
        #[case] desc: &str,
        #[case] id: &str,
        #[case] user: &str,
    ) {
        let tmp = agent_testkit::tempdir();
        let f = factory(tmp.as_path().to_path_buf());
        let r = row(id, user, "acme__web", "github", "");
        assert!(f.build(&r).await.is_err(), "{desc}");
        // Nothing escaped the fleet root.
        assert!(
            !tmp.as_path().parent().unwrap().join("repo").exists(),
            "{desc}: no dir created outside the fleet root"
        );
    }
}
