//! I1 store contracts, executed unchanged by memory and PostgreSQL tiers.
use super::{fixture_v1, fixture_v2, key_alpha, Harness};
use agent_core::repo_graph::*;

pub async fn run(h: &Harness, case: &str) {
    let store = h.a();
    let repo = store
        .repo_put(&RepoSpec {
            slug: "owner__repo".into(),
            forge: "github".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let begin = SnapshotBegin {
        repo,
        commit_sha: "a".repeat(40),
        extractors: vec!["cargo".into()],
        extractor_version: "cargo@1;cfg=1".into(),
    };
    let graph = fixture_v1();
    let report = ExtractReport {
        files: 3,
        skipped: 1,
        parse_errors: 2,
        truncated: true,
        ..Default::default()
    };
    match case {
        "positive_atomic_race" => {
            let (a, b) = tokio::join!(
                store.snapshot_publish(&begin, &graph, &report),
                store.snapshot_publish(&begin, &graph, &report)
            );
            let (a, b) = (a.unwrap(), b.unwrap());
            assert_eq!(a.snapshot.id, b.snapshot.id);
            assert_ne!(a.reused, b.reused);
            assert_eq!(store.snapshots(repo, 200).await.unwrap().len(), 1);
        }
        "positive_report_roundtrip" => {
            let p = store
                .snapshot_publish(&begin, &graph, &report)
                .await
                .unwrap();
            assert_eq!(
                store
                    .snapshot_report(Scope::new(repo, p.snapshot.id))
                    .await
                    .unwrap(),
                Some(report)
            );
            assert_eq!(
                store
                    .snapshot_find_identity(&begin)
                    .await
                    .unwrap()
                    .unwrap()
                    .id,
                p.snapshot.id
            );
            let changed = SnapshotBegin {
                extractor_version: "cargo@1;cfg=2".into(),
                ..begin
            };
            assert!(store
                .snapshot_find_identity(&changed)
                .await
                .unwrap()
                .is_none());
        }
        "corner_legacy_building" => {
            let id = store.snapshot_begin(&begin).await.unwrap();
            assert_eq!(
                store.snapshot_report(Scope::new(repo, id)).await.unwrap(),
                None
            );
            assert!(matches!(
                store.snapshot_publish(&begin, &graph, &report).await,
                Err(RepoGraphError::Conflict(_))
            ));
            store
                .snapshot_recover(Scope::new(repo, id), "stopped legacy writer")
                .await
                .unwrap();
            let failed = store.snapshots(repo, 200).await.unwrap();
            assert_eq!(failed[0].reason, "stopped legacy writer");
            assert_eq!(failed[0].status, SnapshotStatus::Failed);
            assert!(store
                .snapshot_publish(&begin, &graph, &report)
                .await
                .is_ok());
        }
        "boundary_retention" => {
            let first = store
                .snapshot_publish(&begin, &graph, &report)
                .await
                .unwrap()
                .snapshot;
            let view = store.read_view(Scope::new(repo, first.id)).await.unwrap();
            h.advance_secs(1);
            let second = store
                .snapshot_publish(
                    &SnapshotBegin {
                        commit_sha: "b".repeat(40),
                        ..begin
                    },
                    &fixture_v2(),
                    &report,
                )
                .await
                .unwrap()
                .snapshot;
            assert_eq!(store.snapshot_delete_older_than(repo, 1).await.unwrap(), 1);
            assert_eq!(view.shape().await.unwrap().snapshot.id, first.id);
            assert_eq!(view.report().await.unwrap(), Some(report.clone()));
            assert!(!view.nodes_by_key(&[key_alpha()]).await.unwrap().is_empty());
            drop(view);
            assert_eq!(
                store.snapshot_latest(repo).await.unwrap().unwrap().id,
                second.id
            );
            assert!(store.read_view(Scope::new(repo, first.id)).await.is_err());
        }
        "adversarial_wrong_scope" => {
            let p = store
                .snapshot_publish(&begin, &graph, &report)
                .await
                .unwrap();
            let scope = Scope::new(repo, p.snapshot.id);
            assert!(matches!(
                h.b().snapshot_report(scope).await,
                Err(RepoGraphError::NotFound)
            ));
            assert!(h.b().read_view(scope).await.is_err());
            assert!(h.b().snapshot_recover(scope, "stopped").await.is_err());
            assert!(store.snapshot_recover(scope, "\x1b[31m").await.is_err());
            assert!(store.snapshot_recover(scope, "stopped").await.is_err());
            assert_eq!(
                store.snapshot_latest(repo).await.unwrap().unwrap().status,
                SnapshotStatus::Ready
            );
        }
        "negative_attempt_diagnostic" => {
            store
                .snapshot_record_failure(&begin, "extraction deadline", &report)
                .await
                .unwrap();
            let snapshots = store.snapshots(repo, 200).await.unwrap();
            assert_eq!(snapshots[0].status, SnapshotStatus::Failed);
            assert_eq!(snapshots[0].reason, "extraction deadline");
            let ready = store
                .snapshot_publish(&begin, &graph, &report)
                .await
                .unwrap();
            store
                .snapshot_record_failure(&begin, "late failure", &report)
                .await
                .unwrap();
            assert_eq!(
                store.snapshot_latest(repo).await.unwrap().unwrap().id,
                ready.snapshot.id
            );
        }
        "negative_publish_failure" => {
            // A graph validated in isolation can still collide with a previously stored body.
            let first = store
                .snapshot_publish(&begin, &graph, &report)
                .await
                .unwrap()
                .snapshot;
            let existing = graph.nodes()[0].node.id;
            let mut builder = GraphBuilder::default().with_id_fn(move |_| existing);
            builder.node(NodeSpec::new(
                NodeKey::parse("file:other.rs").unwrap(),
                "other.rs",
            ));
            let collision = builder.finish().unwrap().graph;
            let next = SnapshotBegin {
                commit_sha: "b".repeat(40),
                ..begin
            };
            assert!(store
                .snapshot_publish(&next, &collision, &report)
                .await
                .is_err());
            assert_eq!(store.snapshots(repo, 200).await.unwrap().len(), 1);
            assert_eq!(
                store.snapshot_latest(repo).await.unwrap().unwrap().id,
                first.id
            );
        }
        _ => panic!("unknown publication contract"),
    }
}
