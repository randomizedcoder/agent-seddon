//! Operator entry point for tenant-bound graph indexing; never exposed as a model tool.
use crate::Config;
use anyhow::Result;
#[cfg(feature = "repo-graph")]
use anyhow::{bail, Context};

/// Check DSN references/build availability without opening a connection.
pub fn validate(cfg: &Config) -> Result<()> {
    cfg.repo_graph.validate().map_err(anyhow::Error::msg)?;
    #[cfg(feature = "repo-graph")]
    if cfg.repo_graph.store == "postgres" {
        let _ = crate::dsn::resolve_dsn_ref(&cfg.repo_graph.dsn_ref)?;
    }
    Ok(())
}

/// Local operator command. The tenant override follows the existing campaign CLI convention.
pub async fn execute(cfg: &Config, args: &[String]) -> Result<String> {
    #[cfg(feature = "repo-graph")]
    {
        enabled::execute(cfg, args).await
    }
    #[cfg(not(feature = "repo-graph"))]
    {
        let _ = (cfg, args);
        anyhow::bail!("agent repo requires the repo-graph build feature")
    }
}

pub const USAGE: &str = "usage: agent [--config PATH] repo <verb> [--tenant SEG] [--repo SLUG]
  add --forge NAME --remote URL [--default-branch main]
  index --sha REV [--root PATH]
  status
  diff --from SNAPSHOT --to SNAPSHOT
  shape [--snapshot ID]
  recover --snapshot ID --reason TEXT --writer-stopped
Uses [repo_graph]; JSON output. Memory storage lasts only for this process.
Recovery requires the operator to stop the legacy writer before retrying.";

#[cfg(feature = "repo-graph")]
mod enabled {
    use super::*;
    use agent_core::repo_graph::*;
    use agent_repo_graph::{index::Indexer, PgRepoGraph};
    use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

    pub async fn open(
        cfg: &Config,
        tenant: &str,
        migrate: bool,
    ) -> Result<Arc<dyn RepoGraphStore>> {
        validate(cfg)?;
        if !agent_core::safe_segment(tenant) {
            bail!("invalid tenant");
        }
        match cfg.repo_graph.store.as_str() {
            "memory" => Ok(Arc::new(MemRepoGraph::new().with_tenant(tenant)?)),
            "postgres" => {
                let dsn = crate::dsn::resolve_dsn_ref(&cfg.repo_graph.dsn_ref)?;
                let store = PgRepoGraph::connect_lazy(&dsn, cfg.repo_graph.pool_max)?
                    .with_tenant(tenant)?;
                if migrate && cfg.repo_graph.migrate_on_start {
                    store.ensure_migrated().await?;
                }
                Ok(Arc::new(store))
            }
            _ => bail!("set [repo_graph] store to memory or postgres"),
        }
    }
    fn id(flags: &BTreeMap<String, String>, key: &str) -> Result<SnapshotId> {
        let value = flags
            .get(key)
            .context("missing snapshot id")?
            .parse::<i64>()
            .context("invalid snapshot id")?;
        if value <= 0 {
            bail!("snapshot id must be positive");
        }
        Ok(SnapshotId(value))
    }
    pub async fn execute(cfg: &Config, args: &[String]) -> Result<String> {
        let verb = args.first().context("missing repo verb")?;
        let allowed: &[&str] = match verb.as_str() {
            "add" => &[
                "--tenant",
                "--repo",
                "--forge",
                "--remote",
                "--default-branch",
            ],
            "index" => &["--tenant", "--repo", "--sha", "--root"],
            "status" => &["--tenant", "--repo"],
            "diff" => &["--tenant", "--repo", "--from", "--to"],
            "shape" => &["--tenant", "--repo", "--snapshot"],
            "recover" => &[
                "--tenant",
                "--repo",
                "--snapshot",
                "--reason",
                "--writer-stopped",
            ],
            _ => bail!("unknown repo verb"),
        };
        let mut flags = BTreeMap::new();
        let mut it = args[1..].iter();
        while let Some(key) = it.next() {
            if !allowed.contains(&key.as_str()) {
                bail!("unknown repo flag");
            }
            let value = if key == "--writer-stopped" {
                "true".into()
            } else {
                it.next().context("missing repo flag value")?.clone()
            };
            if value.len() > 4096
                || value.chars().any(char::is_control)
                || flags.insert(key.clone(), value).is_some()
            {
                bail!("invalid or duplicate repo flag");
            }
        }
        let tenant = flags
            .get("--tenant")
            .cloned()
            .unwrap_or_else(|| agent_core::current_tenant().as_str().to_string());
        let slug = flags.get("--repo").unwrap_or(&cfg.repo_graph.repo);
        if !agent_core::safe_segment(slug) {
            bail!("repo requires a path-safe slug");
        }
        let store = open(cfg, &tenant, true).await?;
        let output = if verb == "add" {
            let remote = flags.get("--remote").context("add requires --remote")?;
            agent_repo_graph::index::remote_identity(remote)?;
            let spec = RepoSpec {
                slug: slug.clone(),
                forge: flags
                    .get("--forge")
                    .context("add requires --forge")?
                    .clone(),
                remote_url: remote.clone(),
                default_branch: flags
                    .get("--default-branch")
                    .cloned()
                    .unwrap_or_else(|| "main".into()),
                ..Default::default()
            };
            serde_json::json!({"repo_id":store.repo_put(&spec).await?,"tenant":tenant})
        } else {
            let repo = store
                .repo_get(slug)
                .await?
                .context("repository is not registered in this tenant")?;
            match verb.as_str() {
                "index" => {
                    let root = PathBuf::from(flags.get("--root").map(String::as_str).unwrap_or(
                        if cfg.agent.working_dir.is_empty() {
                            "."
                        } else {
                            &cfg.agent.working_dir
                        },
                    ))
                    .canonicalize()
                    .context("index root unavailable")?;
                    let agent_dir = root.join(".agent-seddon");
                    if agent_dir.exists() || agent_dir.symlink_metadata().is_ok() {
                        agent_core::confine(&root, ".agent-seddon").map_err(|_| {
                            anyhow::anyhow!("index state directory is not confined")
                        })?;
                    } else {
                        std::fs::create_dir(&agent_dir)
                            .context("creating index state directory")?;
                    }
                    let worktrees = agent_dir.join("worktrees");
                    if worktrees.exists() || worktrees.symlink_metadata().is_ok() {
                        agent_core::confine(&root, ".agent-seddon/worktrees").map_err(|_| {
                            anyhow::anyhow!("index worktree directory is not confined")
                        })?;
                    } else {
                        std::fs::create_dir(&worktrees)
                            .context("creating index worktree directory")?;
                    }
                    let git = agent_git::CliBackend::new(
                        &root,
                        root.join(".agent-seddon/mirror-unused"),
                        worktrees,
                        "",
                    );
                    let indexer = Indexer::new(
                        store.clone(),
                        Arc::new(git),
                        root,
                        Arc::new(agent_sandbox::LocalSandbox),
                        cfg.repo_graph.index_options(),
                        crate::Metrics::new(),
                    )?;
                    serde_json::to_value(
                        indexer
                            .index(slug, flags.get("--sha").context("index requires --sha")?)
                            .await?,
                    )?
                }
                "status" => {
                    let mut rows = Vec::new();
                    for snapshot in store.snapshots(repo.id, MAX_SNAPSHOT_LIST).await? {
                        let report = store
                            .snapshot_report(Scope::new(repo.id, snapshot.id))
                            .await?;
                        rows.push(serde_json::json!({"manual_recovery_required":snapshot.status==SnapshotStatus::Building,"snapshot":snapshot,"report":report}));
                    }
                    serde_json::json!({"repo_id":repo.id,"snapshots":rows})
                }
                "diff" => {
                    let (a, b) = (id(&flags, "--from")?, id(&flags, "--to")?);
                    // Scope both inputs before calling the legacy id-only diff.
                    store.snapshot_report(Scope::new(repo.id, a)).await?;
                    store.snapshot_report(Scope::new(repo.id, b)).await?;
                    serde_json::to_value(store.snapshot_diff(a, b).await?)?
                }
                "shape" => {
                    let sid = if flags.contains_key("--snapshot") {
                        id(&flags, "--snapshot")?
                    } else {
                        store
                            .snapshot_latest(repo.id)
                            .await?
                            .context("no ready snapshot")?
                            .id
                    };
                    serde_json::to_value(
                        store
                            .read_view(Scope::new(repo.id, sid))
                            .await?
                            .shape()
                            .await?,
                    )?
                }
                "recover" => {
                    if !flags.contains_key("--writer-stopped") {
                        bail!("recover requires --writer-stopped after stopping the writer");
                    }
                    store
                        .snapshot_recover(
                            Scope::new(repo.id, id(&flags, "--snapshot")?),
                            flags.get("--reason").context("recover requires --reason")?,
                        )
                        .await?;
                    serde_json::json!({"recovered":true})
                }
                _ => unreachable!(),
            }
        };
        Ok(serde_json::to_string_pretty(&output)?)
    }
}

#[cfg(feature = "repo-graph")]
pub use enabled::open;

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn corner_repo_index_default_disabled() {
        let cfg = Config::minimal_for_test();
        assert!(validate(&cfg).is_ok());
        assert!(cfg.repo_graph.store.is_empty());
    }
    #[rstest]
    #[case::adversarial_unknown_store("grpc")]
    #[case::negative_typo_store("postgress")]
    fn negative_repo_index_backend(#[case] name: &str) {
        let mut cfg = Config::minimal_for_test();
        cfg.repo_graph.store = name.into();
        assert!(validate(&cfg).is_err());
    }
    #[cfg(feature = "repo-graph")]
    #[tokio::test]
    async fn positive_repo_index_memory_registration() {
        let mut cfg = Config::minimal_for_test();
        cfg.repo_graph.store = "memory".into();
        let result = execute(
            &cfg,
            &[
                "add",
                "--repo",
                "o__r",
                "--tenant",
                "acme",
                "--forge",
                "github",
                "--remote",
                "https://github.com/o/r.git",
            ]
            .map(String::from),
        )
        .await
        .unwrap();
        assert!(result.contains("acme"));
        assert!(result.contains("repo_id"));
        let store = open(&cfg, "acme", false).await.unwrap();
        assert!(
            store.repos().await.unwrap().is_empty(),
            "memory stores are process-handle local"
        );
    }
    #[cfg(feature = "repo-graph")]
    #[rstest]
    #[case::adversarial_tenant(&["status","--repo","o__r","--tenant","../x"])]
    #[case::adversarial_slug(&["status","--repo","../x"])]
    #[case::negative_unknown_flag(&["status","--repo","o__r","--typo","x"])]
    #[case::corner_duplicate(&["status","--repo","o__r","--repo","o__r"])]
    #[case::boundary_value(&["status","--repo"])]
    #[tokio::test]
    async fn negative_repo_index_flags(#[case] args: &[&str]) {
        let mut cfg = Config::minimal_for_test();
        cfg.repo_graph.store = "memory".into();
        assert!(
            execute(&cfg, &args.iter().map(|x| (*x).into()).collect::<Vec<_>>())
                .await
                .is_err()
        );
    }
    #[cfg(feature = "repo-graph")]
    #[test]
    fn boundary_repo_index_config_roundtrip() {
        let input="[agent]\nprovider='openai-compat'\n[provider]\nmodel='test'\n[repo_graph]\nstore='memory'\nrepo='o__r'\nextractors=['cargo','rust']\nretention=2\nmax_files=10\n";
        let (cfg, unknown) = crate::parse_config_reporting_unknown(input).unwrap();
        assert!(unknown.is_empty());
        assert_eq!(cfg.repo_graph.retention, 2);
        assert_eq!(cfg.repo_graph.index_options().max_files, 10);
        assert!(crate::parse_config_reporting_unknown(
            &input.replace("['cargo','rust']", "['scip']")
        )
        .is_err());
    }
    #[cfg(feature = "repo-graph")]
    #[tokio::test]
    #[ignore = "requires AGENT_REPO_GRAPH_TEST_DSN"]
    async fn positive_pg_repo_index_operator_workflow() {
        use std::{path::Path, process::Command};
        fn git(root: &Path, args: &[&str]) {
            assert!(Command::new("git")
                .args(["-c", "core.hooksPath=/dev/null"])
                .args(args)
                .current_dir(root)
                .output()
                .unwrap()
                .status
                .success());
        }
        let root = agent_testkit::tempdir();
        git(&root, &["init", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.invalid"]);
        std::fs::write(root.join("README.md"), "# Test\n").unwrap();
        std::fs::write(root.join(".gitignore"), ".agent-seddon/\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "A"]);
        git(
            &root,
            &["remote", "add", "origin", "https://github.com/o/r.git"],
        );
        let mut cfg = Config::minimal_for_test();
        cfg.repo_graph.store = "postgres".into();
        cfg.repo_graph.repo = "o__r".into();
        cfg.repo_graph.dsn_ref = "env:AGENT_REPO_GRAPH_TEST_DSN".into();
        cfg.agent.working_dir = root.display().to_string();
        let tenant = format!("rk06-cli-{}", uuid::Uuid::new_v4());
        let command = |args: &[&str]| {
            let mut v = args.iter().map(ToString::to_string).collect::<Vec<_>>();
            v.extend(["--tenant".into(), tenant.clone()]);
            v
        };
        execute(
            &cfg,
            &command(&[
                "add",
                "--forge",
                "github",
                "--remote",
                "https://github.com/o/r.git",
            ]),
        )
        .await
        .unwrap();
        let result = execute(&cfg, &command(&["index", "--sha", "HEAD"]))
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result["publication"]["snapshot"]["status"], "ready");
        let shape = execute(&cfg, &command(&["shape"])).await.unwrap();
        assert!(shape.contains("graph_hash"));
        let status = execute(&cfg, &command(&["status"])).await.unwrap();
        assert!(status.contains("manual_recovery_required"));
        let result = execute(&cfg, &command(&["index", "--sha", "HEAD"]))
            .await
            .unwrap();
        assert!(
            serde_json::from_str::<serde_json::Value>(&result).unwrap()["publication"]["reused"]
                .as_bool()
                .unwrap()
        );
        // A tracked escaping state-directory symlink is refused before Git can create worktrees there.
        #[cfg(unix)]
        {
            std::fs::remove_dir_all(root.join(".agent-seddon")).unwrap();
            let outside = agent_testkit::tempdir();
            std::os::unix::fs::symlink(&outside, root.join(".agent-seddon")).unwrap();
            assert!(execute(&cfg, &command(&["index", "--sha", "HEAD"]))
                .await
                .is_err());
            std::fs::remove_dir_all(outside).unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
