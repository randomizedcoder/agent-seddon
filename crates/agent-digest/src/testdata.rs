//! Deterministic, realistic digest corpora for tests and benches.
//!
//! Shapes mirror what the distiller actually writes (increment 02 spec): summaries
//! on the section-locked template, facts as terse bullet lists, periodic
//! `alternatives` and `objective` rows, phase-appropriate keywords. Everything is
//! a pure function of `(session, seq)` — **no randomness, no clock** — so
//! iai-callgrind counts are reproducible and a test failure replays exactly.
//!
//! SQLite is the ephemeral harness of choice here: [`populated_sqlite`] builds an
//! in-memory ledger in microseconds and drops with the test — nothing to clean up.

use agent_core::{Digest, DigestKind};

/// A coding session moves through phases; the corpus shifts content and keywords
/// with it (this is what makes objective-conditioned filtering testable).
const PHASES: &[(&str, &str, &[&str])] = &[
    (
        "explore",
        "map the crate layout and find the seam registration points",
        &["exploration", "registry", "seams", "layout"],
    ),
    (
        "implement",
        "implement the DigestStore seam and its sqlite backend",
        &["digeststore", "sqlite", "seam", "backend"],
    ),
    (
        "debug",
        "fix the failing adversarial tests around id validation",
        &["debugging", "safe-segment", "validation", "tests"],
    ),
    (
        "document",
        "write the component doc and wire the flake checks",
        &["docs", "nix", "bench", "flake"],
    ),
];

fn phase(seq: u64) -> (&'static str, &'static str, &'static [&'static str]) {
    let p = PHASES[(seq as usize / 4) % PHASES.len()];
    (p.0, p.1, p.2)
}

/// The section-locked summary the distiller writes (opencode template) — realistic
/// length (~1 KB) and shape, varying by phase and seq.
pub fn summary_text(session: &str, seq: u64) -> String {
    let (name, objective, _) = phase(seq);
    format!(
        "## Objective\n\
         {objective} (session {session}, exchange {seq})\n\
         ## Important Details\n\
         - crates/agent-digest/src/sqlite.rs holds the {name} work for this span\n\
         - the `(session_id, seq, kind)` primary key is the replace target\n\
         - error mapping goes through `Error::Memory`, never a panic\n\
         ## Work State\n\
         ### Completed\n\
         - exchange {prev} landed the previous slice and its tests\n\
         ### Active\n\
         - exchange {seq}: {objective}\n\
         ### Blocked\n\
         - none\n\
         ## Next Move\n\
         1. finish the {name} slice\n\
         2. run `nix develop -c cargo test -p agent-digest`\n\
         ## Relevant Files\n\
         - crates/agent-digest/src/lib.rs\n\
         - crates/agent-digest/src/sqlite.rs:{line}\n",
        prev = seq.saturating_sub(1),
        line = 40 + (seq % 200),
    )
}

/// The terse facts row for the same exchange.
pub fn facts_text(seq: u64) -> String {
    let (_, objective, _) = phase(seq);
    format!(
        "- decision: {objective}\n\
         - constraint: query limits are capped server-side at 512\n\
         - value fixed at exchange {seq}: text cap = 16 KiB, keywords = 16 x 64B\n"
    )
}

/// One realistic digest row. `kind` decides the text shape; keywords follow the
/// session phase (plus a per-kind tag so kind-blind keyword queries are testable).
pub fn digest(session: &str, seq: u64, kind: DigestKind) -> Digest {
    let (phase_name, objective, words) = phase(seq);
    let text = match kind {
        DigestKind::Summary => summary_text(session, seq),
        DigestKind::Facts => facts_text(seq),
        DigestKind::Objective => format!("Current objective: {objective}"),
        DigestKind::Alternatives => format!(
            "1. keep-{phase_name}: stay the course. Reconsider when: exchange {seq} \
             uncovers a blocker\n\
             2. alternative path: switch approach. Reconsider when: the {phase_name} \
             phase stalls twice\n"
        ),
    };
    let mut keywords: Vec<String> = words.iter().map(|w| (*w).to_string()).collect();
    keywords.push(kind.as_str().to_string());
    Digest {
        session_id: session.to_string(),
        user_id: "local".to_string(),
        seq,
        kind,
        text,
        keywords,
        mode: if phase_name == "explore" {
            "design".to_string()
        } else {
            phase_name.to_string()
        },
        model: "kimi".to_string(),
        // Fixed epoch base + 90s per exchange — deterministic, plausibly spaced.
        ts_ms: 1_700_000_000_000 + seq * 90_000,
        duration_ms: 800 + (seq % 7) as u32 * 150,
        tokens: 180 + (seq % 5) as u32 * 40,
    }
}

/// The full ledger of an `exchanges`-turn session: one summary + one facts row per
/// exchange, an `alternatives` row every 5th, an `objective` row every 8th
/// (compaction happened) — the realistic row mix.
pub fn session_rows(session: &str, exchanges: u64) -> Vec<Digest> {
    let mut rows = Vec::new();
    for seq in 1..=exchanges {
        rows.push(digest(session, seq, DigestKind::Summary));
        rows.push(digest(session, seq, DigestKind::Facts));
        if seq % 5 == 0 {
            rows.push(digest(session, seq, DigestKind::Alternatives));
        }
        if seq % 8 == 0 {
            rows.push(digest(session, seq, DigestKind::Objective));
        }
    }
    rows
}

/// An ephemeral, fully-populated in-memory sqlite ledger: `sessions` sessions
/// (`s0`, `s1`, …) × `exchanges` turns each. Create, test, drop — no cleanup.
#[cfg(feature = "digest-sqlite")]
pub fn populated_sqlite(sessions: u64, exchanges: u64) -> crate::SqliteDigests {
    let store = crate::SqliteDigests::in_memory().expect("in-memory ledger");
    populate(&store, sessions, exchanges);
    store
}

/// Populate the sqlite store with the standard corpus. Runtime-free: the sqlite
/// backend's put is synchronous under the hood (`Mutex<Connection>`), exposed as
/// [`crate::SqliteDigests::put_sync`] precisely so fixtures and benches need no
/// executor.
#[cfg(feature = "digest-sqlite")]
fn populate(store: &crate::SqliteDigests, sessions: u64, exchanges: u64) {
    for s in 0..sessions {
        for row in session_rows(&format!("s{s}"), exchanges) {
            store.put_sync(row).expect("populate put");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_corpus_is_deterministic_and_phase_shaped() {
        let a = session_rows("s0", 20);
        let b = session_rows("s0", 20);
        assert_eq!(a.len(), b.len());
        assert!(a
            .iter()
            .zip(&b)
            .all(|(x, y)| x.text == y.text && x.keywords == y.keywords && x.ts_ms == y.ts_ms));
        // 20 exchanges → 20 summaries + 20 facts + 4 alternatives + 2 objectives.
        assert_eq!(a.len(), 46);
        // Phase shift is visible in the keywords (what relevance filtering keys on).
        let early = &a[0]; // seq 1 → explore
        let late = a.iter().find(|d| d.seq == 6).unwrap(); // seq 6 → implement
        assert!(early.keywords.contains(&"exploration".to_string()));
        assert!(late.keywords.contains(&"sqlite".to_string()));
    }

    #[test]
    fn positive_summary_text_is_section_locked() {
        let t = summary_text("s0", 3);
        for section in [
            "## Objective",
            "## Important Details",
            "## Work State",
            "### Completed",
            "### Active",
            "### Blocked",
            "## Next Move",
            "## Relevant Files",
        ] {
            assert!(t.contains(section), "missing {section}:\n{t}");
        }
    }

    #[cfg(feature = "digest-sqlite")]
    #[tokio::test]
    async fn positive_populated_sqlite_round_trips_the_corpus() {
        use agent_core::{DigestQuery, DigestStore};
        let store = populated_sqlite(2, 12);
        let q = DigestQuery {
            session_id: "s1".into(),
            kind: Some(DigestKind::Summary),
            ..DigestQuery::default()
        };
        let rows = store.query(&q).await.unwrap();
        assert_eq!(rows.len(), 12);
        assert_eq!(rows[0].seq, 1);
        assert!(rows[11].text.contains("## Objective"));
    }
}
