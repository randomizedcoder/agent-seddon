//! dhat heap gate for the registry hot paths (run under `--features dhat-heap`;
//! wired into `nix/checks/leak.nix`): sustained `Put`/`Delete` churn and `Route`
//! introspection must free everything they allocate and stay under a per-op
//! allocation budget — a control plane that leaks per mutation dies exactly when
//! it is being used as designed (many agents driving one registry).

#![cfg(feature = "dhat-heap")]

use agent_core::{PoolTier, ProviderRegistry, RouteHint, RouteRole, Upstream};
use agent_registry::MemoryRegistry;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn card(id: &str) -> Upstream {
    Upstream {
        id: id.into(),
        kind: "openai-compat".into(),
        enabled: true,
        base_url: "http://127.0.0.1:1/v1".into(),
        model: "m".into(),
        api_key_ref: "env:K".into(),
        context_window: 128_000,
        tags: vec!["reasoning".into(), "cheap".into()],
        tier: Some(PoolTier::Medium),
        ..Default::default()
    }
}

#[test]
fn put_delete_route_churn_frees_everything() {
    let profiler = dhat::Profiler::builder().testing().build();
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("rt");

    rt.block_on(async {
        let reg = MemoryRegistry::empty();
        // A resident fleet the churn happens against.
        for i in 0..16 {
            reg.put(card(&format!("base{i}"))).await.expect("seed");
        }

        let before = dhat::HeapStats::get();
        const OPS: usize = 200;
        for i in 0..OPS {
            let id = format!("churn{}", i % 4);
            reg.put(card(&id)).await.expect("put");
            let d = reg
                .route(&RouteHint {
                    role: Some(RouteRole::Judge),
                    ..Default::default()
                })
                .await
                .expect("route");
            assert!(!d.chosen.is_empty());
            reg.delete(&id).await.expect("delete");
        }
        let after = dhat::HeapStats::get();

        // Steady state: the churn left no net blocks behind (the resident fleet
        // itself is untouched).
        assert!(
            after.curr_blocks <= before.curr_blocks,
            "net blocks grew: {} -> {}",
            before.curr_blocks,
            after.curr_blocks
        );
        // Allocation budget per put+route+delete cycle: bounded scratch, not
        // an accumulation. Generous ceiling; the point is the order of magnitude.
        let per_op = (after.total_blocks - before.total_blocks) / OPS as u64;
        assert!(per_op < 400, "allocation budget blown: {per_op} blocks/op");
    });

    drop(profiler);
}
