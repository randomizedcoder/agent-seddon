//! Shared ClickHouse **reader** plumbing for the tenant-scoped read seams — the
//! fleet review history (C16, [`crate::history`]) and cross-session recall (C28-3,
//! [`crate::recall`]). Both embed a [`ChReader`] so the lazy-connect,
//! reconnect-once-on-error, and per-connection C27 RLS tenant `SET` discipline
//! lives in exactly one place.
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

/// A lazily-connected ClickHouse reader: shares the `[telemetry]` connection
/// params with the writer (one server; the writer inserts, this reads back), and
/// — when `tenant_scoped` — binds each fresh connection to the caller's verified
/// tenant via the C27 RLS `SET`.
pub(crate) struct ChReader {
    /// `host:port` for the native protocol (e.g. `localhost:9000`).
    addr: String,
    database: String,
    user: String,
    password: String,
    /// When true (a distinct `agent_reader` credential was provisioned, C27), each
    /// fresh connection issues `SET SQL_tenant_id = <verified identity>` so the
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

    /// Engage per-tenant RLS scoping (C27): each connection will `SET SQL_tenant_id`
    /// from the verified ambient identity. Chainable; on only when a distinct
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
        // C27: bind this connection to the caller's tenant so the server-side ROW
        // POLICY prunes every other tenant's rows. Sourced from the *verified*
        // ambient identity (never a model payload); `set_tenant_stmt` fails closed
        // on an absent/hostile identity (no SET ⇒ the policy's `''` default ⇒ no
        // tenant rows).
        if self.tenant_scoped {
            let tenant = agent_core::current_identity()
                .map(|k| k.user.as_str().to_string())
                .unwrap_or_default();
            if let Some(stmt) = set_tenant_stmt(&tenant) {
                client.execute(stmt.as_str()).await.map_err(ch_err)?;
            }
        }
        Ok(client)
    }

    /// Fail-closed liveness check: lazily connect (reusing the cached client,
    /// reconnecting once if stale) and run a trivial `SELECT 1` round-trip.
    pub(crate) async fn ping(&self) -> Result<()> {
        self.with_client(|client| async move { client.execute("SELECT 1").await })
            .await
    }

    /// Run `op` on the cached client; on error, reconnect once and retry (a
    /// restarted ClickHouse heals on the next call). Mirrors the digest store's
    /// discipline.
    pub(crate) async fn with_client<T, F, Fut>(&self, op: F) -> Result<T>
    where
        F: Fn(Client) -> Fut,
        Fut: std::future::Future<Output = klickhouse::Result<T>>,
    {
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            *guard = Some(self.connect().await?);
        }
        let client = guard.clone().expect("client just ensured");
        match op(client).await {
            Ok(v) => Ok(v),
            Err(first) => {
                *guard = None; // stale connection — rebuild and retry once
                let fresh = self.connect().await.map_err(|e| {
                    Error::Memory(format!("clickhouse: {first}; reconnect failed: {e}"))
                })?;
                let v = op(fresh.clone()).await.map_err(ch_err)?;
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
}
