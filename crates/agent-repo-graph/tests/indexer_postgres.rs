#![cfg(all(feature = "index", feature = "repo-graph-postgres"))]
mod common;
use agent_core::repo_graph::*;
use agent_repo_graph::{index::IndexOptions, PgRepoGraph};
use common::*;
use std::sync::Arc;

async fn stores(tenant: &str) -> (Arc<PgRepoGraph>, Arc<PgRepoGraph>, sqlx::PgPool) {
    let dsn = std::env::var("AGENT_REPO_GRAPH_TEST_DSN").expect("required PostgreSQL DSN missing");
    let a = PgRepoGraph::connect(&dsn, 8, true)
        .await
        .unwrap()
        .with_tenant(tenant)
        .unwrap();
    let b = PgRepoGraph::connect(&dsn, 8, false)
        .await
        .unwrap()
        .with_tenant(tenant)
        .unwrap();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&dsn)
        .await
        .unwrap();
    sqlx::query("DELETE FROM repos WHERE tenant=$1")
        .bind(tenant)
        .execute(&pool)
        .await
        .unwrap();
    (Arc::new(a), Arc::new(b), pool)
}
#[tokio::test]
#[ignore = "requires AGENT_REPO_GRAPH_TEST_DSN"]
async fn positive_pg_pipeline_restart() {
    let git = GitFixture::new();
    let (a, b, _) = stores("rk06-restart").await;
    let repo = git.register(a.as_ref()).await;
    let first = git
        .indexer(a, IndexOptions::default())
        .index("owner__repo", "HEAD")
        .await
        .unwrap();
    let again = git
        .indexer(b.clone(), IndexOptions::default())
        .index("owner__repo", &git.a)
        .await
        .unwrap();
    assert!(again.publication.reused);
    assert_eq!(first.publication.snapshot.id, again.publication.snapshot.id);
    let next = git.change();
    let second = git
        .indexer(b.clone(), IndexOptions::default())
        .index("owner__repo", &next)
        .await
        .unwrap();
    assert!(!b
        .snapshot_diff(
            first.publication.snapshot.id,
            second.publication.snapshot.id
        )
        .await
        .unwrap()
        .added
        .is_empty());
    assert_eq!(
        b.snapshot_report(Scope::new(repo, first.publication.snapshot.id))
            .await
            .unwrap(),
        first.report
    );
    // A third revision remains readable after another handle is dropped/reopened.
    std::fs::write(git.root.join("README.md"), "# Revision C\n").unwrap();
    common::git(&git.root, &["add", "."]);
    common::git(&git.root, &["commit", "-m", "C"]);
    let third = common::git(&git.root, &["rev-parse", "HEAD"]);
    let c = git
        .indexer(b, IndexOptions::default())
        .index("owner__repo", &third)
        .await
        .unwrap();
    assert_eq!(c.publication.snapshot.commit_sha, third);
}
#[tokio::test]
#[ignore = "requires AGENT_REPO_GRAPH_TEST_DSN"]
async fn corner_pg_concurrent_publish() {
    let git = GitFixture::new();
    let (a, b, _) = stores("rk06-race").await;
    let repo = git.register(a.as_ref()).await;
    let ia = git.indexer(a.clone(), IndexOptions::default());
    let ib = git.indexer(b, IndexOptions::default());
    let (x, y) = tokio::join!(
        ia.index("owner__repo", "HEAD"),
        ib.index("owner__repo", "HEAD")
    );
    let (x, y) = (x.unwrap(), y.unwrap());
    assert_eq!(x.publication.snapshot.id, y.publication.snapshot.id);
    assert_ne!(x.publication.reused, y.publication.reused);
    assert_eq!(
        a.snapshots(repo, 200)
            .await
            .unwrap()
            .iter()
            .filter(|x| x.status == SnapshotStatus::Ready)
            .count(),
        1
    );
}
#[tokio::test]
#[ignore = "requires AGENT_REPO_GRAPH_TEST_DSN"]
async fn negative_pg_disconnect() {
    let git = GitFixture::new();
    let (a, b, pool) = stores("rk06-disconnect").await;
    let repo = git.register(a.as_ref()).await;
    // Terminate the actual publisher after shared body inserts, before publication finishes.
    sqlx::raw_sql("CREATE OR REPLACE FUNCTION rk06_disconnect() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_terminate_backend(pg_backend_pid()); RETURN NEW; END $$; DROP TRIGGER IF EXISTS rk06_disconnect ON graph_node_versions; CREATE TRIGGER rk06_disconnect BEFORE INSERT ON graph_node_versions FOR EACH ROW WHEN (NEW.tenant = 'rk06-disconnect') EXECUTE FUNCTION rk06_disconnect();").execute(&pool).await.unwrap();
    let result = git
        .indexer(a, IndexOptions::default())
        .index("owner__repo", "HEAD")
        .await;
    sqlx::raw_sql(
        "DROP TRIGGER rk06_disconnect ON graph_node_versions; DROP FUNCTION rk06_disconnect();",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(result.is_err());
    let attempts = b.snapshots(repo, 200).await.unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].status, SnapshotStatus::Failed);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM graph_nodes WHERE tenant='rk06-disconnect'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    assert!(git
        .indexer(b, IndexOptions::default())
        .index("owner__repo", "HEAD")
        .await
        .is_ok());
}
#[tokio::test]
#[ignore = "requires AGENT_REPO_GRAPH_TEST_DSN"]
async fn boundary_pg_retention_read() {
    let git = GitFixture::new();
    let (a, b, _) = stores("rk06-retention").await;
    let repo = git.register(a.as_ref()).await;
    let first = git
        .indexer(a.clone(), IndexOptions::default())
        .index("owner__repo", "HEAD")
        .await
        .unwrap();
    let view = a
        .read_view(Scope::new(repo, first.publication.snapshot.id))
        .await
        .unwrap();
    git.change();
    let next = git
        .indexer(
            b.clone(),
            IndexOptions {
                retention: 1,
                ..Default::default()
            },
        )
        .index("owner__repo", "HEAD")
        .await
        .unwrap();
    assert_eq!(next.pruned, 1);
    assert_eq!(view.report().await.unwrap(), first.report);
    assert_eq!(
        view.shape().await.unwrap().snapshot.id,
        first.publication.snapshot.id
    );
    assert!(!view
        .nodes_by_file(&["src/lib.rs".into()])
        .await
        .unwrap()
        .is_empty());
    drop(view);
    assert!(b
        .read_view(Scope::new(repo, first.publication.snapshot.id))
        .await
        .is_err());
}
