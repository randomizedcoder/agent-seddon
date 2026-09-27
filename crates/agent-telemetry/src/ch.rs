//! Shared ClickHouse **reader** plumbing for the tenant-scoped read seams — the
//! fleet review history (C16, [`crate::history`]) and cross-session recall (C28-3,
//! [`crate::recall`]). Both embed a [`ChReader`] so the lazy-connect,
//! reconnect-once-on-error, and per-read C27 RLS tenant binding discipline
//! lives in exactly one place.
//!
//! The tenant is bound **before every tenant-data read**, not once per connection
//! (security-hardening S16): the cached connection is shared process-wide, so a
//! connection first bound to tenant A must never serve tenant B's read under A's
//! scope. A scoped read with no verified tenant is refused rather than run on
//! whatever scope the connection last carried.
//!
//! **Durable read, like the digest store** (unlike the fire-and-forget telemetry
//! writer): lazily connects over the native protocol, reconnects once on a stale
//! connection, and surfaces errors so the caller can fall back.

use agent_core::{Error, Result};
use klickhouse::{Client, ClientOptions};
use tokio::sync::Mutex;

/// Map a klickhouse error into the seam error type used by both readers.
pub(crate) fn ch_err(e: klickhouse::KlickhouseError) -> Error {
    Error::Memory(format!("clickhouse: {e}"))
}

/// The custom ClickHouse setting the C27 `tenant_iso_*` ROW POLICYs read
/// (`USING user = getSetting('SQL_tenant_id')`). The `SQL_` prefix is declared
/// server-side via `<custom_settings_prefixes>` (nix/clickhouse/users.xml); the
/// `agent_reader` profile locks it read-only with a `''` default so an unset
/// connection sees no tenant-owned rows.
const TENANT_SETTING: &str = "SQL_tenant_id";

/// The `SET SQL_tenant_id = '<tenant>'` statement that binds an `agent_reader`
/// connection to one tenant's rows via the server-side ROW POLICY — or `None`
/// when there is no safe tenant to set (empty, or `safe_segment`-rejected).
///
/// **Fail closed:** with no valid setting the policy's `''` default matches only
/// unowned rows, so a hostile or absent identity reads nothing rather than
/// everything. `safe_segment`'s charset (`[A-Za-z0-9._-]`) contains no quote,
/// `;`, or whitespace, so the single-quoted literal cannot be broken out of — the
/// tenant value can never inject SQL. Pure so it is table-testable without a live
/// ClickHouse (the enforcement itself is a live-only test).
pub(crate) fn set_tenant_stmt(tenant: &str) -> Option<String> {
    if !agent_core::safe_segment(tenant) {
        return None;
    }
    Some(format!("SET {TENANT_SETTING} = '{tenant}'"))
}

/// The RLS scope to bind before one tenant-data read: `Ok(None)` when the reader is
/// not tenant-scoped (Tier 0, the writer credential sees every row anyway), the
/// `SET` for a valid verified tenant, and an error when scoped but no valid tenant is
/// in scope. **Fail closed:** the error stops the read instead of letting it run on
/// the tenant the shared connection was last bound to. Pure so it is table-testable.
pub(crate) fn read_scope(tenant_scoped: bool, tenant: Option<&str>) -> Result<Option<String>> {
    if !tenant_scoped {
        return Ok(None);
    }
    tenant.and_then(set_tenant_stmt).map(Some).ok_or_else(|| {
        Error::Memory("clickhouse: no verified tenant in scope for a tenant-scoped read".into())
    })
}

/// Run `op` on `client` after binding it to `scope` (when there is one). The bind and
/// the read run back to back on one connection while the caller holds the reader's
/// lock, so no other read can re-bind the connection in between.
async fn bound<T, F, Fut>(client: Client, scope: Option<&str>, op: &F) -> klickhouse::Result<T>
where
    F: Fn(Client) -> Fut,
    Fut: std::future::Future<Output = klickhouse::Result<T>>,
{
    if let Some(stmt) = scope {
        client.execute(stmt).await?;
    }
    op(client).await
}

/// A lazily-connected ClickHouse reader: shares the `[telemetry]` connection
/// params with the writer (one server; the writer inserts, this reads back), and
/// — when `tenant_scoped` — binds every tenant-data read to the caller's verified
/// tenant via the C27 RLS `SET`.
pub(crate) struct ChReader {
    /// `host:port` for the native protocol (e.g. `localhost:9000`).
    addr: String,
    database: String,
    user: String,
    password: String,
    /// When true (a distinct `agent_reader` credential was provisioned, C27), every
    /// tenant-data read first issues `SET SQL_tenant_id = <verified identity>` so the
    /// server-side ROW POLICY scopes every read to the caller's tenant. False at
    /// Tier 0 (writer credential reused, no policy) ⇒ byte-for-byte today's behaviour.
    tenant_scoped: bool,
    /// Lazily-connected, dropped on error so the next op reconnects.
    client: Mutex<Option<Client>>,
}

impl ChReader {
    pub(crate) fn new(
        addr: impl Into<String>,
        database: impl Into<String>,
        user: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            addr: addr.into(),
            database: database.into(),
            user: user.into(),
            password: password.into(),
            tenant_scoped: false,
            client: Mutex::new(None),
        }
    }

    /// Engage per-tenant RLS scoping (C27): every tenant-data read will
    /// `SET SQL_tenant_id` from the verified ambient identity. Chainable; on only when a distinct
    /// `agent_reader` credential is configured.
    #[must_use]
    pub(crate) fn tenant_scoped(mut self, yes: bool) -> Self {
        self.tenant_scoped = yes;
        self
    }

    pub(crate) fn database(&self) -> &str {
        &self.database
    }

    async fn connect(&self) -> Result<Client> {
        let client = Client::connect(
            self.addr.as_str(),
            ClientOptions {
                username: self.user.clone(),
                password: self.password.clone(),
                default_database: self.database.clone(),
                tcp_nodelay: true,
            },
        )
        .await
        .map_err(ch_err)?;
        client
            .execute("SET log_queries = 0, log_query_threads = 0")
            .await
            .map_err(ch_err)?;
        Ok(client)
    }

    /// Fail-closed liveness check: lazily connect (reusing the cached client,
    /// reconnecting once if stale) and run a trivial `SELECT 1` round-trip.
    pub(crate) async fn ping(&self) -> Result<()> {
        self.with_client_unscoped(|client| async move { client.execute("SELECT 1").await })
            .await
    }

    /// Run a **tenant-data** read `op` on the cached client, bound first to the
    /// caller's verified tenant when the reader is tenant-scoped (see [`read_scope`]);
    /// on error, reconnect once and retry (a restarted ClickHouse heals on the next
    /// call). Mirrors the digest store's discipline.
    pub(crate) async fn with_client<T, F, Fut>(&self, op: F) -> Result<T>
    where
        F: Fn(Client) -> Fut,
        Fut: std::future::Future<Output = klickhouse::Result<T>>,
    {
        let scope = read_scope(self.tenant_scoped, agent_core::scoped_tenant().as_deref())?;
        self.run(scope.as_deref(), op).await
    }

    /// Run `op` with no tenant binding: only for tenant-agnostic operations (the
    /// liveness ping, the `system.tables` schema-drift check), never a read of a
    /// tenant-bearing table.
    pub(crate) async fn with_client_unscoped<T, F, Fut>(&self, op: F) -> Result<T>
    where
        F: Fn(Client) -> Fut,
        Fut: std::future::Future<Output = klickhouse::Result<T>>,
    {
        self.run(None, op).await
    }

    async fn run<T, F, Fut>(&self, scope: Option<&str>, op: F) -> Result<T>
    where
        F: Fn(Client) -> Fut,
        Fut: std::future::Future<Output = klickhouse::Result<T>>,
    {
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            *guard = Some(self.connect().await?);
        }
        let client = guard.clone().expect("client just ensured");
        match bound(client, scope, &op).await {
            Ok(v) => Ok(v),
            Err(first) => {
                *guard = None; // stale connection — rebuild and retry once
                let fresh = self.connect().await.map_err(|e| {
                    Error::Memory(format!("clickhouse: {first}; reconnect failed: {e}"))
                })?;
                let v = bound(fresh.clone(), scope, &op).await.map_err(ch_err)?;
                *guard = Some(fresh);
                Ok(v)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// desc: `set_tenant_stmt` — the per-connection RLS scope statement (C27). A valid
    /// tenant yields `SET SQL_tenant_id = '<tenant>'`; an absent/hostile one yields
    /// `None` (fail closed — the ROW POLICY's `''` default then matches no tenant-owned
    /// rows). The value can never inject: `safe_segment`'s charset carries no
    /// quote/`;`/whitespace.
    #[rstest]
    #[case::positive_plain("acme", Some("SET SQL_tenant_id = 'acme'"))]
    #[case::positive_org_review("agent-seddon", Some("SET SQL_tenant_id = 'agent-seddon'"))]
    #[case::positive_dotted("org.team_1", Some("SET SQL_tenant_id = 'org.team_1'"))]
    #[case::negative_empty("", None)]
    #[case::adversarial_single_quote("a' OR '1'='1", None)]
    #[case::adversarial_statement_break("a'; DROP TABLE agent.agent_events; --", None)]
    #[case::adversarial_newline("a\nb", None)]
    #[case::adversarial_space("a b", None)]
    #[case::adversarial_traversal("..", None)]
    #[case::adversarial_leading_dash("-x", None)]
    #[case::adversarial_backtick("a`b", None)]
    fn set_tenant_stmt_scopes_or_fails_closed(#[case] tenant: &str, #[case] expect: Option<&str>) {
        assert_eq!(set_tenant_stmt(tenant).as_deref(), expect);
    }

    /// desc: boundary on the identity length — exactly `MAX_SEGMENT_LEN` is accepted and
    /// quoted verbatim; one char over is rejected (fail closed). Computed, not a literal,
    /// so the case can't silently desync from the cap.
    #[test]
    fn boundary_set_tenant_stmt_at_and_over_max_len() {
        let at = "a".repeat(agent_core::MAX_SEGMENT_LEN);
        assert_eq!(
            set_tenant_stmt(&at),
            Some(format!("SET SQL_tenant_id = '{at}'")),
            "exactly MAX_SEGMENT_LEN is a valid tenant"
        );
        let over = "a".repeat(agent_core::MAX_SEGMENT_LEN + 1);
        assert_eq!(
            set_tenant_stmt(&over),
            None,
            "one over the cap fails closed (no SET emitted)"
        );
    }

    /// desc: `read_scope` — the per-read RLS binding (S16). Unscoped readers bind
    /// nothing; a scoped reader binds the verified tenant, and refuses (an error, not
    /// an empty or stale scope) when no valid tenant is in scope — so a shared
    /// connection last bound to another tenant can never serve the read.
    #[rstest]
    #[case::positive_scoped_tenant(true, Some("acme"), Ok(Some("SET SQL_tenant_id = 'acme'")))]
    #[case::positive_unscoped_binds_nothing(false, Some("acme"), Ok(None))]
    #[case::corner_unscoped_without_tenant_binds_nothing(false, None, Ok(None))]
    #[case::negative_scoped_without_tenant_refused(true, None, Err(()))]
    #[case::negative_scoped_empty_tenant_refused(true, Some(""), Err(()))]
    #[case::adversarial_scoped_injection_refused(true, Some("a' OR '1'='1"), Err(()))]
    #[case::adversarial_scoped_traversal_refused(true, Some(".."), Err(()))]
    fn read_scope_binds_or_refuses(
        #[case] scoped: bool,
        #[case] tenant: Option<&str>,
        #[case] expect: std::result::Result<Option<&str>, ()>,
    ) {
        let got = read_scope(scoped, tenant);
        match expect {
            Ok(want) => assert_eq!(got.expect("in scope").as_deref(), want),
            Err(()) => {
                let msg = got.expect_err("must refuse").to_string();
                // The refusal never echoes the offending value.
                assert!(!msg.contains("OR"), "echoed the tenant: {msg}");
            }
        }
    }

    /// desc: boundary — a tenant exactly at the segment cap binds; one over refuses.
    #[test]
    fn boundary_read_scope_at_and_over_max_len() {
        let at = "a".repeat(agent_core::MAX_SEGMENT_LEN);
        assert!(read_scope(true, Some(&at)).unwrap().is_some());
        let over = "a".repeat(agent_core::MAX_SEGMENT_LEN + 1);
        assert!(read_scope(true, Some(&over)).is_err());
    }

    /// One `user` value from `agent_events` (the live RLS test's probe row).
    #[derive(Debug, klickhouse::Row)]
    struct UserRow {
        user: String,
    }

    async fn users_seen(reader: &ChReader) -> Result<Vec<String>> {
        let rows: Vec<UserRow> = reader
            .with_client(|client| async move {
                client
                    .query_collect::<UserRow>(
                        "SELECT DISTINCT user FROM agent.agent_events ORDER BY user",
                    )
                    .await
            })
            .await?;
        Ok(rows.into_iter().map(|r| r.user).collect())
    }

    fn scoped(tenant: &str) -> agent_core::SessionKey {
        agent_core::SessionKey::parse(tenant, "s1").expect("valid identity")
    }

    /// desc: live (opt-in, `nix run .#ch-integration`) — `boundary_two_tenants_share_one_connection`:
    /// one `agent_reader` `ChReader` serves tenant A then tenant B on its single cached
    /// connection; B must see only B's rows (before S16 the connection kept A's `SET`
    /// and B read A's rows). An unscoped read on the same connection is refused. The
    /// harness seeds `agent_events` with rows for tenants `rls-a` and `rls-b`.
    #[tokio::test]
    #[ignore = "needs a live ClickHouse; run via `nix run .#ch-integration`"]
    async fn boundary_two_tenants_share_one_connection() {
        let addr = std::env::var("AGENT_CH_RLS_TEST_ADDR").expect("AGENT_CH_RLS_TEST_ADDR");
        let password = std::env::var("AGENT_CH_RLS_TEST_READER_PASSWORD")
            .expect("AGENT_CH_RLS_TEST_READER_PASSWORD");
        let reader = ChReader::new(addr, "agent", "agent_reader", password).tenant_scoped(true);

        let a = agent_core::scope(scoped("rls-a"), users_seen(&reader)).await;
        assert_eq!(a.expect("tenant A reads"), vec!["rls-a".to_string()]);
        let b = agent_core::scope(scoped("rls-b"), users_seen(&reader)).await;
        assert_eq!(
            b.expect("tenant B reads"),
            vec!["rls-b".to_string()],
            "tenant B must not read under tenant A's cached scope"
        );
        assert!(
            users_seen(&reader).await.is_err(),
            "an unscoped read on a tenant-scoped reader is refused"
        );
        reader
            .ping()
            .await
            .expect("tenant-agnostic ping needs no tenant");
    }

    /// One `(event, reason, subject)` from `agent_auth_events`.
    #[derive(Debug, klickhouse::Row)]
    struct AuthProbeRow {
        event: String,
        reason: String,
        subject: String,
    }

    /// desc: live (opt-in, `nix run .#ch-integration`) — `positive_auth_events_written_and_tenant_read`:
    /// the telemetry writer (as `agent_writer`) inserts `agent_auth_events` rows whose
    /// `LowCardinality` columns come from plain `String` fields; `agent_reader` reads
    /// back only its own tenant's row (security-hardening S11).
    #[tokio::test]
    #[ignore = "needs a live ClickHouse; run via `nix run .#ch-integration`"]
    async fn positive_auth_events_written_and_tenant_read() {
        use agent_core::{AuthEvent, AuthEventKind};
        let addr = std::env::var("AGENT_CH_RLS_TEST_ADDR").expect("AGENT_CH_RLS_TEST_ADDR");
        let writer_password = std::env::var("AGENT_CH_RLS_TEST_WRITER_PASSWORD")
            .expect("AGENT_CH_RLS_TEST_WRITER_PASSWORD");
        let reader_password = std::env::var("AGENT_CH_RLS_TEST_READER_PASSWORD")
            .expect("AGENT_CH_RLS_TEST_READER_PASSWORD");
        let handle = crate::TelemetryHandle::spawn(
            crate::TelemetryConfig {
                addr: addr.clone(),
                database: "agent".into(),
                user: "agent_writer".into(),
                password: writer_password,
                batch_max_rows: 100,
                flush_interval: std::time::Duration::from_millis(50),
            },
            "s1",
        );
        for tenant in ["rls-w1", "rls-w2"] {
            handle.record_auth_event(AuthEvent {
                tenant: tenant.into(),
                subject: format!("user:kc/{tenant}"),
                action: "approve",
                resource_type: "review",
                reason: "missing_permission",
                ..AuthEvent::new(AuthEventKind::AuthzDeny)
            });
        }
        handle.shutdown().await;

        let reader =
            ChReader::new(addr, "agent", "agent_reader", reader_password).tenant_scoped(true);
        let rows: Vec<AuthProbeRow> = agent_core::scope(scoped("rls-w1"), async {
            reader
                .with_client(|client| async move {
                    client
                        .query_collect::<AuthProbeRow>(
                            "SELECT event, reason, subject FROM agent.agent_auth_events \
                             WHERE user LIKE 'rls-w%'",
                        )
                        .await
                })
                .await
        })
        .await
        .expect("tenant read");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(
            (
                rows[0].event.as_str(),
                rows[0].reason.as_str(),
                rows[0].subject.as_str()
            ),
            ("authz_deny", "missing_permission", "user:kc/rls-w1")
        );
    }
}
