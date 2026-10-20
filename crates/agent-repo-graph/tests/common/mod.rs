use agent_core::repo_graph::*;
use agent_metrics::Metrics;
use agent_repo_graph::index::{IndexOptions, Indexer};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

pub struct GitFixture {
    pub dir: PathBuf,
    pub root: PathBuf,
    pub remote: String,
    pub a: String,
}
impl GitFixture {
    pub fn new() -> Self {
        let dir = agent_testkit::tempdir();
        let root = dir.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&dir, &["init", "--bare", "origin.git"]);
        git(&root, &["init", "-b", "main"]);
        git(&root, &["config", "user.email", "test@example.invalid"]);
        git(&root, &["config", "user.name", "Test"]);
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        std::fs::write(root.join(".gitignore"), ".agent-seddon/\n").unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "A"]);
        let remote = format!("file://{}", dir.join("origin.git").display());
        git(&root, &["remote", "add", "origin", &remote]);
        let a = git(&root, &["rev-parse", "HEAD"]);
        Self {
            dir,
            root,
            remote,
            a,
        }
    }
    pub async fn register(&self, store: &dyn RepoGraphStore) -> RepoId {
        store
            .repo_put(&RepoSpec {
                slug: "owner__repo".into(),
                forge: "github".into(),
                remote_url: self.remote.clone(),
                ..Default::default()
            })
            .await
            .unwrap()
    }
    pub fn indexer(&self, store: Arc<dyn RepoGraphStore>, options: IndexOptions) -> Indexer {
        Indexer::new(
            store,
            Arc::new(agent_git::CliBackend::new(
                &self.root,
                self.dir.join("absent-mirror"),
                self.dir.join("worktrees"),
                "",
            )),
            self.root.clone(),
            Arc::new(agent_sandbox::LocalSandbox),
            options,
            Metrics::new(),
        )
        .unwrap()
    }
    pub fn change(&self) -> String {
        std::fs::write(
            self.root.join("src/lib.rs"),
            "pub fn alpha() { let changed = 2; }\npub fn beta() {}\n",
        )
        .unwrap();
        git(&self.root, &["add", "."]);
        git(&self.root, &["commit", "-m", "B"]);
        git(&self.root, &["rev-parse", "HEAD"])
    }
}
impl Drop for GitFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
pub fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git fixture failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
