//! `FleetRegistry` seam implementations (review-fleet C2): the durable **roster**
//! of "who reviews what" — the list of review sessions the fleet server admits and
//! drives.
//!
//! Three backends, selected by config like every other seam (mirroring the
//! `agent-registry` triad):
//! - [`MemoryFleet`] — in-process, the base for tests and the shape a serve-only
//!   process falls back to with no backing file configured.
//! - [`FileFleet`] — one JSON bundle on disk (`Vec<FleetSession>`), hand-editable
//!   *or* rewritten by a control-plane `Put`; an absent file is an empty roster.
//! - `SqliteFleet` (feature `fleet-sqlite`) — each row as JSON in an embedded
//!   SQLite BLOB.
//!
//! **Untrusted input, fail closed.** Every id may become a storage-path segment;
//! every row arrives from a gRPC peer or a hand-edited file. Stores validate ids
//! ([`safe_segment`]), clamp hostile numbers ([`FleetSession::sanitize`]), and cap
//! counts *before* anything is persisted. `token_ref` is stored and served verbatim
//! as a **reference** — no store ever resolves it (there is no token to leak).

use agent_core::{safe_segment, Error, FleetRegistry, FleetSession, Result, MAX_FLEET_ROWS};
use async_trait::async_trait;
use std::sync::Mutex;

pub mod file;
pub use file::FileFleet;
#[cfg(feature = "fleet-sqlite")]
pub mod sqlite;
#[cfg(feature = "fleet-sqlite")]
pub use sqlite::SqliteFleet;

/// Shared fail-closed id gate for lookups: a hostile id is rejected before it is
/// compared (and before it could reach a storage path in any backend).
fn check_id(id: &str) -> Result<()> {
    if safe_segment(id) {
        Ok(())
    } else {
        Err(Error::Fleet("invalid fleet session id".into()))
    }
}

fn not_found(id: &str) -> Error {
    // The `not found` prefix is the seam contract: the wire layer maps it to gRPC
    // NotFound (id is safe to echo — it passed `check_id`).
    Error::Fleet(format!("not found: fleet session `{id}`"))
}

/// Apply one CRUD mutation to a roster snapshot. All three backends route their
/// writes through these, so validation/clamps/caps can never drift between them.
mod ops {
    use super::*;

    pub fn put(rows: &mut Vec<FleetSession>, mut session: FleetSession) -> Result<FleetSession> {
        session.sanitize();
        session.validate()?;
        match rows.iter_mut().find(|r| r.id == session.id) {
            Some(slot) => *slot = session.clone(),
            None => {
                if rows.len() >= MAX_FLEET_ROWS {
                    return Err(Error::Fleet(format!(
                        "fleet roster is full ({MAX_FLEET_ROWS} sessions)"
                    )));
                }
                rows.push(session.clone());
            }
        }
        Ok(session)
    }

    pub fn delete(rows: &mut Vec<FleetSession>, id: &str) -> Result<bool> {
        check_id(id)?;
        let before = rows.len();
        rows.retain(|r| r.id != id);
        Ok(rows.len() != before)
    }

    pub fn set_enabled(rows: &mut [FleetSession], id: &str, enabled: bool) -> Result<FleetSession> {
        check_id(id)?;
        let row = rows
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or_else(|| not_found(id))?;
        row.enabled = enabled;
        Ok(row.clone())
    }

    /// Re-validate a whole loaded roster fail-closed (defends against out-of-band
    /// edits to a file/row): a single bad row makes the whole read fail, never a
    /// partially-loaded roster.
    pub fn revalidate(rows: &[FleetSession]) -> Result<()> {
        for r in rows {
            r.validate()?;
        }
        Ok(())
    }
}

/// The in-process [`FleetRegistry`]: one roster snapshot behind a mutex. The base
/// for tests, and what a serve-only process runs with no backing file configured.
#[derive(Default)]
pub struct MemoryFleet {
    rows: Mutex<Vec<FleetSession>>,
}

impl MemoryFleet {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<FleetSession>> {
        // A poisoned lock means a panic mid-mutation; the snapshot is still a
        // consistent value (mutations build the new state before storing it).
        self.rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[async_trait]
impl FleetRegistry for MemoryFleet {
    async fn list(&self) -> Result<Vec<FleetSession>> {
        Ok(self.lock().clone())
    }
    async fn get(&self, id: &str) -> Result<FleetSession> {
        check_id(id)?;
        self.lock()
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or_else(|| not_found(id))
    }
    async fn put(&self, session: FleetSession) -> Result<FleetSession> {
        ops::put(&mut self.lock(), session)
    }
    async fn delete(&self, id: &str) -> Result<bool> {
        ops::delete(&mut self.lock(), id)
    }
    async fn set_enabled(&self, id: &str, enabled: bool) -> Result<FleetSession> {
        ops::set_enabled(&mut self.lock(), id, enabled)
    }
}

#[cfg(test)]
pub(crate) mod testdata {
    use super::*;

    /// A well-formed roster row keyed by `id` (owner `acme`, GitHub, env-ref token).
    pub fn row(id: &str) -> FleetSession {
        FleetSession {
            id: id.into(),
            user: "acme".into(),
            repo: "acme__widget".into(),
            backend: "github".into(),
            base_url: String::new(),
            token_ref: "env:FLEET_TOKEN".into(),
            skill: "review".into(),
            slack_trigger_channel: "C_TRIGGER".into(),
            slack_progress_channel: "C_PROGRESS".into(),
            poll_secs: 300,
            enabled: true,
            created_at: 1,
            updated_at: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testdata::row;
    use super::*;
    use agent_core::{
        DEFAULT_FLEET_POLL_SECS, MAX_FLEET_POLL_SECS, MAX_SEGMENT_LEN, MIN_FLEET_POLL_SECS,
    };
    use rstest::rstest;

    /// One CRUD mutation, exercised against a store pre-seeded with an enabled
    /// `row("seed")`.
    enum Op {
        // Boxed: a `FleetSession` dwarfs the other (id-only) variants, so an
        // unboxed payload would bloat every `Op` (clippy::large_enum_variant).
        Put(Box<FleetSession>),
        Delete(&'static str),
        SetEnabled(&'static str, bool),
        Get(&'static str),
    }

    /// The coarse observable outcome of an [`Op`] the CRUD table asserts on.
    #[derive(Debug, PartialEq, Eq)]
    enum Expect {
        Ok,
        DeletedTrue,
        DeletedFalse,
        Err,
        NotFound,
    }

    fn classify(r: Result<()>) -> Expect {
        match r {
            Ok(()) => Expect::Ok,
            Err(e) if e.to_string().contains("not found") => Expect::NotFound,
            Err(_) => Expect::Err,
        }
    }

    async fn outcome(store: &MemoryFleet, op: Op) -> Expect {
        match op {
            Op::Put(s) => classify(store.put(*s).await.map(|_| ())),
            Op::Get(id) => classify(store.get(id).await.map(|_| ())),
            Op::SetEnabled(id, en) => classify(store.set_enabled(id, en).await.map(|_| ())),
            Op::Delete(id) => match store.delete(id).await {
                Ok(true) => Expect::DeletedTrue,
                Ok(false) => Expect::DeletedFalse,
                Err(_) => Expect::Err,
            },
        }
    }

    /// A `row("x")` with its id overridden — for the id-shape cases.
    fn put_with_id(id: String) -> Op {
        let mut r = row("x");
        r.id = id;
        Op::Put(Box::new(r))
    }

    // --- CRUD contract: one table over every op class, `desc` + `expect` per row ---

    #[rstest]
    #[case::positive_get_present("get an existing row", Op::Get("seed"), Expect::Ok)]
    #[case::positive_delete_present(
        "delete an existing row ⇒ true",
        Op::Delete("seed"),
        Expect::DeletedTrue
    )]
    #[case::positive_set_enabled_present(
        "toggle an existing row",
        Op::SetEnabled("seed", false),
        Expect::Ok
    )]
    #[case::positive_put_new("insert a fresh row", Op::Put(Box::new(row("fresh"))), Expect::Ok)]
    #[case::corner_put_upserts_existing(
        "re-put same id upserts, still Ok",
        Op::Put(Box::new(row("seed"))),
        Expect::Ok
    )]
    #[case::negative_get_unknown(
        "get a missing id ⇒ not found",
        Op::Get("ghost"),
        Expect::NotFound
    )]
    #[case::negative_set_enabled_unknown(
        "toggle a missing id ⇒ not found",
        Op::SetEnabled("ghost", true),
        Expect::NotFound
    )]
    #[case::negative_delete_unknown(
        "delete a missing id ⇒ Ok(false)",
        Op::Delete("ghost"),
        Expect::DeletedFalse
    )]
    #[case::boundary_id_at_max("id exactly at MAX_SEGMENT_LEN is accepted", put_with_id("a".repeat(MAX_SEGMENT_LEN)), Expect::Ok)]
    #[case::boundary_id_over_max("id one past MAX_SEGMENT_LEN is rejected", put_with_id("a".repeat(MAX_SEGMENT_LEN + 1)), Expect::Err)]
    #[case::adversarial_traversal_id("a traversal id is rejected", put_with_id("../escape".into()), Expect::Err)]
    #[tokio::test]
    async fn crud_contract(#[case] desc: &str, #[case] op: Op, #[case] expect: Expect) {
        let store = MemoryFleet::new();
        store.put(row("seed")).await.expect("seed");
        assert_eq!(outcome(&store, op).await, expect, "{desc}");
    }

    #[tokio::test]
    async fn positive_list_returns_enabled_and_disabled() {
        let store = MemoryFleet::new();
        store.put(row("on")).await.unwrap();
        let mut off = row("off");
        off.enabled = false;
        store.put(off).await.unwrap();
        let all = store.list().await.unwrap();
        assert_eq!(all.len(), 2, "the roster view keeps disabled rows");
        assert!(all.iter().any(|r| r.id == "off" && !r.enabled));
    }

    #[tokio::test]
    async fn negative_raw_token_in_token_ref_refused_and_not_echoed() {
        let store = MemoryFleet::new();
        let mut bad = row("r1");
        bad.token_ref = "ghp_rawsecrettoken".into();
        let err = store.put(bad).await.expect_err("raw token rejected");
        assert!(
            !err.to_string().contains("ghp_rawsecret"),
            "the error must never echo the secret: {err}"
        );
    }

    // --- Boundary: poll clamp + id length ------------------------------------

    #[rstest]
    #[case::below_floor_clamps_up(MIN_FLEET_POLL_SECS - 1, MIN_FLEET_POLL_SECS)]
    #[case::at_floor_kept(MIN_FLEET_POLL_SECS, MIN_FLEET_POLL_SECS)]
    #[case::at_ceiling_kept(MAX_FLEET_POLL_SECS, MAX_FLEET_POLL_SECS)]
    #[case::over_ceiling_clamps_down(MAX_FLEET_POLL_SECS + 1, MAX_FLEET_POLL_SECS)]
    #[case::zero_becomes_default(0, DEFAULT_FLEET_POLL_SECS)]
    #[tokio::test]
    async fn boundary_poll_secs_clamped_to_bounds(#[case] input: u64, #[case] expect: u64) {
        let store = MemoryFleet::new();
        let mut r = row("r1");
        r.poll_secs = input;
        let stored = store.put(r).await.expect("put clamps poll_secs");
        assert_eq!(stored.poll_secs, expect, "poll_secs {input} ⇒ {expect}");
    }

    // --- Corner: upsert-in-place + empty optionals ---------------------------

    #[tokio::test]
    async fn corner_put_same_id_updates_in_place() {
        let store = MemoryFleet::new();
        store.put(row("r1")).await.unwrap();
        let mut upd = row("r1");
        upd.skill = "deep-review".into();
        store.put(upd).await.unwrap();
        assert_eq!(
            store.list().await.unwrap().len(),
            1,
            "upsert, not duplicate"
        );
        assert_eq!(store.get("r1").await.unwrap().skill, "deep-review");
    }

    #[tokio::test]
    async fn corner_empty_optional_fields_ok() {
        let store = MemoryFleet::new();
        let mut r = row("r1");
        r.base_url = String::new();
        r.token_ref = String::new(); // unauthenticated
        r.backend = String::new(); // resolve as a registered forge
        r.slack_trigger_channel = String::new();
        r.slack_progress_channel = String::new();
        assert!(store.put(r).await.is_ok(), "blank optionals are valid");
    }

    // --- Adversarial: traversal ids + rows cap -------------------------------

    #[rstest]
    #[case::parent_traversal("../../etc/passwd")]
    #[case::separator("a/b")]
    #[case::leading_dash("-rf")]
    #[case::dot(".")]
    #[case::dotdot("..")]
    #[case::empty("")]
    #[tokio::test]
    async fn adversarial_traversal_id_rejected_everywhere(#[case] id: &str) {
        let store = MemoryFleet::new();
        store.put(row("keep")).await.unwrap();
        assert!(store.get(id).await.is_err(), "get {id:?}");
        assert!(store.delete(id).await.is_err(), "delete {id:?}");
        assert!(
            store.set_enabled(id, true).await.is_err(),
            "set_enabled {id:?}"
        );
        let mut bad = row("ok");
        bad.id = id.into();
        assert!(store.put(bad).await.is_err(), "put {id:?}");
        assert_eq!(
            store.list().await.unwrap().len(),
            1,
            "no rejected call mutated the roster"
        );
    }

    #[tokio::test]
    async fn adversarial_over_rows_cap_rejected() {
        let store = MemoryFleet::new();
        for i in 0..MAX_FLEET_ROWS {
            store.put(row(&format!("r{i}"))).await.expect("fits");
        }
        assert!(
            store.put(row("one-too-many")).await.is_err(),
            "insert past the cap is rejected"
        );
        // Updating an existing row is not an insert.
        let mut upd = row("r0");
        upd.skill = "x".into();
        assert!(
            store.put(upd).await.is_ok(),
            "upsert of an existing id still allowed at cap"
        );
    }

    #[tokio::test]
    async fn adversarial_unknown_backend_rejected() {
        let store = MemoryFleet::new();
        let mut r = row("r1");
        r.backend = "evil-forge".into();
        assert!(
            store.put(r).await.is_err(),
            "only github/gitlab/'' accepted"
        );
    }
}
