//! Deterministic instruction-count bench for instant compaction's assembly path —
//! the whole point of increment 03 is that this path is cheap, and the ceiling
//! keeps it so. Measured: one over-budget `compact` in `Relevance::Keyword` mode
//! over a 40-exchange corpus ledger (sqlite in-memory, the ephemeral harness) —
//! ledger fetch + decode, keyword relevance, section assembly, rebuild — with the
//! single objective LLM call scripted (constant). See
//! docs/design/cognition-graph/03-instant-compaction.md.

use std::hint::black_box;
use std::sync::Arc;

use agent_context::{InstantCfg, InstantWindow, Relevance};
use agent_core::{
    ContextStrategy, Message, SessionId, SessionKey, TokenBudget, UserId, WorkingSet,
};
use agent_digest::testdata;
use agent_testkit::{final_turn, ScriptedProvider};
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

struct Fixture {
    window: InstantWindow,
    working: WorkingSet,
    rt: tokio::runtime::Runtime,
}

fn fixture() -> Fixture {
    // A realistic long session: compaction only helps (reduces tokens) when the
    // working set genuinely dwarfs the assembled ledger block. A short (~40-exchange)
    // corpus's block — objective + summaries + the facts/alternatives sections — is
    // actually larger than the raw messages, so the token-budget accept check (rightly)
    // rejects it. 120 exchanges is a session where assembly is the win it's meant to be.
    let store = Arc::new(testdata::populated_sqlite(1, 120));
    let provider = Arc::new(ScriptedProvider::new(vec![final_turn(
        "Implement the DigestStore seam and its sqlite backend.",
    )]));
    let window = InstantWindow::new(provider, store, 64).with_cfg(InstantCfg {
        relevance: Relevance::Keyword,
        ..InstantCfg::default()
    });
    let mut messages = vec![Message::system("you are an agent")];
    for i in 1..=120u32 {
        messages.push(Message::user(format!("exchange {i}: {}", "x".repeat(120))));
        messages.push(Message::assistant(format!("done {i}: {}", "y".repeat(120))));
    }
    Fixture {
        window,
        working: WorkingSet { messages },
        rt: tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("rt"),
    }
}

// Observed 5,802,735 Ir (3 ledger queries + per-row injection re-screens +
// section assembly over a 120-exchange corpus — a session long enough that
// assembly actually reduces tokens, so the budget-fit accept check keeps the
// assembly path; the path runs only at compaction). Ceiling ~1.1×. NOTE: the
// in-bench assert below is load-bearing — an early unguarded run silently
// measured the drop-oldest fallback at 64k Ir, and a too-small budget silently
// measures the classic-summarizer fallback instead of assembly.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 6_500_000u64)])))]
#[bench::assemble(setup = fixture)]
fn instant_assemble(mut f: Fixture) -> usize {
    // Window large enough that the assembled block fits (so the assembly path is
    // accepted, not rejected by the fit check), yet well below the 120-exchange
    // working set so compaction triggers.
    let budget = TokenBudget {
        max_context_tokens: 6_000,
        reserve_output: 100,
    };
    let key = SessionKey {
        user: UserId::local(),
        session: SessionId::new("s0"),
    };
    f.rt.block_on(agent_core::scope(key, async {
        f.window
            .compact(&mut f.working, &budget, None)
            .await
            .expect("compact");
    }));
    // Guard the measured path: the ledger assembly must actually have run (a
    // silent fallback would make this bench measure the wrong thing).
    assert!(
        f.working.messages[1]
            .content_text()
            .contains("## Current objective"),
        "bench did not take the instant-assembly path"
    );
    black_box(f.working.messages.len())
}

library_benchmark_group!(name = instant_assemble_group; benchmarks = instant_assemble);
main!(library_benchmark_groups = instant_assemble_group);
