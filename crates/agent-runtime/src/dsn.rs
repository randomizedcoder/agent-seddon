//! Resolve a Postgres DSN *reference* into a connection string, fail-closed.
//!
//! The DSN is a **reference, never a literal**: `dsn_ref` is `env:NAME` or
//! `file:/path` ([`agent_core::DsnRef`]). Resolution is fail-closed — an unset
//! env var or an unreadable/empty file is a hard error (a `postgres` tier cannot
//! start without its DSN) — and **no error message ever echoes the resolved
//! DSN** (it carries a password). The *reference* (var name / path) is operator
//! config and safe to echo.
//!
//! Shared by every Postgres tier: the config-store domains (via
//! [`crate::store_backend`]) and the digest ledger (via the builder's
//! `pg_digests`), so the secret-handling lives in exactly one place.

use anyhow::Context;

/// Resolve `dsn_ref` (`env:NAME` / `file:/path`) into a connection string.
pub(crate) fn resolve_dsn_ref(dsn_ref: &str) -> anyhow::Result<String> {
    use agent_core::DsnRef;
    match DsnRef::parse(dsn_ref).map_err(|e| anyhow::anyhow!(e))? {
        DsnRef::Env(name) => {
            let v = std::env::var(name)
                .map_err(|_| anyhow::anyhow!("[config_store] dsn_ref env var `{name}` is unset"))?;
            if v.is_empty() {
                anyhow::bail!("[config_store] dsn_ref env var `{name}` is empty");
            }
            Ok(v)
        }
        DsnRef::File(path) => {
            let expanded = crate::builder::expand_tilde(path);
            // Never echo the file *contents*; the path itself is operator config.
            let v = std::fs::read_to_string(&expanded)
                .with_context(|| format!("reading [config_store] dsn_ref file `{expanded}`"))?;
            let v = v.trim().to_string();
            if v.is_empty() {
                anyhow::bail!("[config_store] dsn_ref file `{expanded}` is empty");
            }
            Ok(v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // desc: an `env:` ref resolves to the variable's value.
    #[test]
    fn positive_env_ref_resolves() {
        // A unique var name so the test is order-independent.
        let name = "AGENT_A3_TEST_DSN_POSITIVE";
        std::env::set_var(name, "postgres://u:p@127.0.0.1:5432/db");
        let got = resolve_dsn_ref(&format!("env:{name}")).expect("resolves");
        assert_eq!(got, "postgres://u:p@127.0.0.1:5432/db");
        std::env::remove_var(name);
    }

    // negative: an unset env var is a hard error (fail-closed), not a default.
    #[test]
    fn negative_unset_env_is_fail_closed() {
        let err = resolve_dsn_ref("env:AGENT_A3_TEST_DSN_DEFINITELY_UNSET")
            .expect_err("unset var must fail closed");
        assert!(err.to_string().contains("unset"), "{err}");
    }

    // corner: an empty `dsn_ref` is rejected (a postgres backend needs a DSN).
    #[test]
    fn corner_empty_dsn_ref_rejected() {
        assert!(resolve_dsn_ref("").is_err());
    }

    // adversarial: an inline DSN (the secret-in-config mistake) is rejected, and
    // the error never echoes the connection string / password.
    #[test]
    fn adversarial_inline_dsn_rejected_without_echo() {
        let inline = "postgres://admin:hunter2@db.internal:5432/prod";
        let err = resolve_dsn_ref(inline).expect_err("inline DSN must be rejected");
        let msg = err.to_string();
        assert!(!msg.contains("hunter2"), "leaked password: {msg}");
        assert!(!msg.contains("db.internal"), "leaked host: {msg}");
    }
}
