#!/usr/bin/env python3
"""`nix run .#ch-integration` — the ClickHouse credential + row-level-security matrix
(security-hardening S16) against a REAL, throwaway ClickHouse.

It starts its own container (never the long-lived agent ClickHouse) with the repo's
users.xml and a freshly generated credentials directory (ch_creds.py), applies
schema.sql, proves the schema alone leaves no open login, applies the password
ALTERs, seeds two tenants, then asserts who can log in and who sees which rows.
With --cargo it also runs the ignored Rust test that reproduces the shared-connection
scope bug (`ch::tests::boundary_two_tenants_share_one_connection`).

Exit codes (the shared 0/1/2 contract): 0 all passed (or skipped: no container
runtime), 1 a harness failure (the server never came up), 2 a contract failure (a
matrix row did not hold).
"""

from __future__ import annotations

import argparse
import base64
import os
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

sys.path.insert(0, str(Path(__file__).resolve().parent))
import ch_creds  # noqa: E402

TENANT_A = "rls-a"
TENANT_B = "rls-b"


class HarnessError(Exception):
    """The environment failed (exit 1), not the thing under test."""


@dataclass(frozen=True)
class Reply:
    ok: bool
    body: str


# Runs one SQL statement as (user, password or None) and returns the reply.
Query = Callable[[str, "str | None", str], Reply]


def http_query(url: str) -> Query:
    def run(user: str, password: str | None, sql: str) -> Reply:
        req = urllib.request.Request(url + "/", data=sql.encode(), method="POST")
        token = base64.b64encode(f"{user}:{password or ''}".encode()).decode()
        req.add_header("Authorization", f"Basic {token}")
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                return Reply(True, resp.read().decode())
        except urllib.error.HTTPError as e:
            return Reply(False, e.read().decode(errors="replace"))

    return run


@dataclass(frozen=True)
class Case:
    """One matrix row: `sql` run as `user` must succeed with exactly `rows` (sorted
    lines), or fail when `rows` is None."""

    name: str
    user: str
    role: str | None  # whose password to present; None = no password
    sql: str
    rows: tuple[str, ...] | None


def check(case: Case, query: Query, passwords: dict[str, str]) -> str | None:
    """Return a failure message, or None when the row holds."""
    reply = query(case.user, passwords[case.role] if case.role else None, case.sql)
    if case.rows is None:
        return None if not reply.ok else f"expected a refusal, got rows {reply.body!r}"
    if not reply.ok:
        return f"expected rows {list(case.rows)}, got an error: {reply.body.strip()[:160]}"
    got = tuple(sorted(line for line in reply.body.splitlines() if line))
    return None if got == case.rows else f"expected rows {list(case.rows)}, got {list(got)}"


def distinct_users(extra: str = "") -> str:
    return f"SELECT DISTINCT user FROM agent.agent_events{extra}"


def auth_users(extra: str = "") -> str:
    return f"SELECT DISTINCT user FROM agent.agent_auth_events{extra}"


# Before the password ALTERs: the schema alone must leave no open login.
PRE_ALTER = (
    Case("negative_schema_only_reader_cannot_log_in", "agent_reader", None, "SELECT 1", None),
    Case("negative_schema_only_writer_cannot_log_in", "agent_writer", None, "SELECT 1", None),
    Case("negative_schema_only_viewer_cannot_log_in", "agent_viewer", None, "SELECT 1", None),
)

MATRIX = (
    # positive
    Case("positive_reader_sees_own_tenant_rows", "agent_reader", "reader",
         distinct_users(f" SETTINGS SQL_tenant_id = '{TENANT_A}'"), (TENANT_A,)),
    Case("positive_reader_sees_other_bound_tenant_only", "agent_reader", "reader",
         distinct_users(f" SETTINGS SQL_tenant_id = '{TENANT_B}'"), (TENANT_B,)),
    Case("positive_writer_reads_all_tenants", "agent_writer", "writer",
         distinct_users(), (TENANT_A, TENANT_B)),
    Case("positive_viewer_reads_all_tenants", "agent_viewer", "viewer",
         distinct_users(), (TENANT_A, TENANT_B)),
    Case("positive_viewer_reads_default_db", "agent_viewer", "viewer",
         "SELECT x FROM default.rls_probe", ("1",)),
    Case("positive_admin_reads_all_tenants", "default", "admin",
         distinct_users(), (TENANT_A, TENANT_B)),
    Case("positive_reader_sees_own_tenant_auth_events", "agent_reader", "reader",
         auth_users(f" SETTINGS SQL_tenant_id = '{TENANT_A}'"), (TENANT_A,)),
    Case("positive_viewer_reads_every_auth_event", "agent_viewer", "viewer",
         "SELECT count() FROM agent.agent_auth_events", ("3",)),
    Case("positive_writer_inserts", "agent_writer", "writer",
         f"INSERT INTO agent.agent_usage (user) VALUES ('{TENANT_A}')", ()),
    # negative
    Case("negative_no_password_admin_login_fails", "default", None, "SELECT 1", None),
    Case("negative_no_password_writer_login_fails", "agent_writer", None, "SELECT 1", None),
    Case("negative_no_password_reader_login_fails", "agent_reader", None, "SELECT 1", None),
    Case("negative_no_password_viewer_login_fails", "agent_viewer", None, "SELECT 1", None),
    Case("negative_wrong_password_fails", "agent_reader", "writer", "SELECT 1", None),
    Case("negative_empty_tenant_query_fails_closed", "agent_reader", "reader",
         distinct_users(), ()),
    Case("negative_reader_cannot_insert", "agent_reader", "reader",
         f"INSERT INTO agent.agent_events (user) VALUES ('{TENANT_A}')", None),
    Case("negative_viewer_cannot_insert", "agent_viewer", "viewer",
         f"INSERT INTO agent.agent_events (user) VALUES ('{TENANT_A}')", None),
    Case("negative_writer_cannot_manage_access", "agent_writer", "writer",
         "CREATE USER rls_escalate", None),
    # adversarial
    Case("adversarial_reader_cannot_read_default_db", "agent_reader", "reader",
         "SELECT x FROM default.rls_probe", None),
    Case("adversarial_reader_injection_in_setting_matches_nothing", "agent_reader", "reader",
         distinct_users(" SETTINGS SQL_tenant_id = 'rls-a'' OR ''1''=''1'"), ()),
    Case("adversarial_reader_cannot_read_other_tenant_auth_events", "agent_reader", "reader",
         auth_users(f" WHERE user = '{TENANT_B}' SETTINGS SQL_tenant_id = '{TENANT_A}'"), ()),
    # adversarial: rows written outside a scope (user = '') stay hidden from a reader
    # that binds no tenant, or binds the empty one (found live on l2, S18). Counted,
    # since the row matcher drops empty lines.
    Case("adversarial_reader_without_tenant_cannot_read_unscoped_rows", "agent_reader", "reader",
         "SELECT count() FROM agent.agent_logs", ("0",)),
    Case("adversarial_reader_empty_tenant_cannot_read_unscoped_rows", "agent_reader", "reader",
         "SELECT count() FROM agent.agent_logs SETTINGS SQL_tenant_id = ''", ("0",)),
    Case("positive_viewer_reads_unscoped_rows", "agent_viewer", "viewer",
         "SELECT count() FROM agent.agent_logs WHERE user = ''", ("1",)),
    # corner: an unproven refusal ('' tenant) is visible to operators, never to a tenant
    Case("corner_unproven_auth_refusal_hidden_from_reader", "agent_reader", "reader",
         auth_users(" SETTINGS SQL_tenant_id = ''"), ()),
    # corner: a user named in no policy is tenant-blind (users_without_row_policies = false)
    Case("corner_user_without_policy_sees_no_rows", "rls_probe_user", "reader",
         distinct_users(), ()),
    # boundary: the digest table's tenant column is user_id, not user
    Case("boundary_digest_policy_keys_on_user_id", "agent_reader", "reader",
         f"SELECT DISTINCT user_id FROM agent.agent_turn_digests SETTINGS SQL_tenant_id = '{TENANT_B}'",
         (TENANT_B,)),
)

SEED = f"""
INSERT INTO agent.agent_events (user, session_id) VALUES ('{TENANT_A}', 's1'), ('{TENANT_B}', 's1');
-- ts must be recent: the table's 400-day TTL drops a default (1970) row on insert.
INSERT INTO agent.agent_auth_events (ts, user, event, reason) VALUES (now64(3), '{TENANT_A}', 'login', ''), (now64(3), '{TENANT_B}', 'authz_deny', 'missing_permission'), (now64(3), '', 'verify_fail', 'no_token');
INSERT INTO agent.agent_logs (ts, user, session_id, message) VALUES (now64(3), '', '', 'process log'), (now64(3), '{TENANT_A}', 's1', 'tenant log');
INSERT INTO agent.agent_turn_digests (user_id, session_id) VALUES ('{TENANT_A}', 's1'), ('{TENANT_B}', 's1');
CREATE TABLE IF NOT EXISTS default.rls_probe (x UInt8) ENGINE = MergeTree ORDER BY x;
INSERT INTO default.rls_probe VALUES (1);
"""


def probe_user_sql(reader_password: str) -> str:
    """A fresh user granted SELECT but named in no policy (the corner case)."""
    h = ch_creds.sha256_hex(reader_password)
    return (
        f"CREATE USER IF NOT EXISTS rls_probe_user IDENTIFIED WITH sha256_hash BY '{h}' HOST ANY;\n"
        "GRANT SELECT ON agent.* TO rls_probe_user;\n"
    )


def run_matrix(cases, query: Query, passwords: dict[str, str]) -> list[str]:
    failures = []
    for case in cases:
        msg = check(case, query, passwords)
        print(f"  {'ok  ' if msg is None else 'FAIL'} {case.name}" + (f": {msg}" if msg else ""))
        if msg:
            failures.append(case.name)
    return failures


class Container:
    def __init__(self, runtime: str, name: str, image: str, http: int, native: int):
        self.runtime, self.name, self.image = runtime, name, image
        self.http, self.native = http, native

    def _run(self, *args: str, stdin: str | None = None, env: dict | None = None):
        return subprocess.run(
            [self.runtime, *args], input=stdin, text=True, capture_output=True,
            env={**os.environ, **(env or {})},
        )

    def start(self, config_xml: Path, users_xml: Path, server_xml: Path) -> None:
        self.remove()
        # Absolute paths: a relative one would be taken as a named volume.
        config_xml, users_xml, server_xml = (p.resolve() for p in (config_xml, users_xml, server_xml))
        r = self._run(
            "run", "-d", "--name", self.name,
            "-p", f"127.0.0.1:{self.http}:8123", "-p", f"127.0.0.1:{self.native}:9000",
            "-v", f"{config_xml}:/etc/clickhouse-server/config.d/agent.xml:ro",
            "-v", f"{users_xml}:/etc/clickhouse-server/users.d/99-allow-remote-default.xml:ro",
            "-v", f"{server_xml}:/etc/clickhouse-server/users.d/{ch_creds.SERVER_XML}:ro",
            self.image,
        )
        if r.returncode:
            raise HarnessError(f"container did not start: {r.stderr.strip()}")
        for _ in range(90):
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{self.http}/ping", timeout=2) as resp:
                    if resp.read().decode().strip() == "Ok.":
                        return
            except OSError:
                pass
            time.sleep(1)
        raise HarnessError("ClickHouse never answered /ping")

    def admin_sql(self, sql: str, admin_password: str) -> None:
        # The password rides the exec environment, never argv.
        r = self._run(
            "exec", "-i", "-e", "CLICKHOUSE_PASSWORD", self.name,
            "clickhouse-client", "--multiquery",
            stdin=sql, env={"CLICKHOUSE_PASSWORD": admin_password},
        )
        if r.returncode:
            raise HarnessError(f"admin SQL failed: {r.stderr.strip()[:300]}")

    def remove(self) -> None:
        self._run("rm", "-f", self.name)


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--runtime", default=os.environ.get("CONTAINER_RUNTIME", "docker"))
    ap.add_argument("--image", required=True)
    ap.add_argument("--name", required=True)
    ap.add_argument("--http-port", type=int, required=True)
    ap.add_argument("--native-port", type=int, required=True)
    ap.add_argument("--schema", type=Path, required=True)
    ap.add_argument("--config-xml", type=Path, required=True)
    ap.add_argument("--users-xml", type=Path, required=True)
    ap.add_argument("--cargo", action="store_true", help="also run the ignored Rust test")
    args = ap.parse_args(argv)

    if subprocess.run([args.runtime, "info"], capture_output=True).returncode:
        print(f"ch-integration: SKIP — container runtime ({args.runtime}) not reachable.")
        return 0

    ct = Container(args.runtime, args.name, args.image, args.http_port, args.native_port)
    with tempfile.TemporaryDirectory(prefix="ch-rls-") as tmp:
        root = Path(tmp) / "creds"
        try:
            ch_creds.ensure(root)
            pw = {r: ch_creds.read_password(root, r) for r in ch_creds.ROLES}
            ct.start(args.config_xml, args.users_xml, ch_creds.server_xml_path(root))
            ct.admin_sql(args.schema.read_text(), pw["admin"])
            query = http_query(f"http://127.0.0.1:{args.http_port}")
            print("==> ch-integration: schema applied, no passwords set yet")
            failures = run_matrix(PRE_ALTER, query, pw)
            ct.admin_sql(ch_creds.alter_sql(pw), pw["admin"])
            ct.admin_sql(SEED + probe_user_sql(pw["reader"]), pw["admin"])
            print("==> ch-integration: passwords set, two tenants seeded")
            failures += run_matrix(MATRIX, query, pw)
            if args.cargo:
                print("==> ch-integration: the live Rust tests (ignored): shared-connection "
                      "regression, auth-event writer round trip")
                live = ["boundary_two_tenants_share_one_connection",
                        "positive_auth_events_written_and_tenant_read"]
                rc = subprocess.run(
                    ["nix", "develop", "--extra-experimental-features", "nix-command flakes",
                     "-c", "cargo", "test", "-p", "agent-telemetry", "--lib", "--",
                     "--ignored", "--exact", *(f"ch::tests::{t}" for t in live)],
                    env={**os.environ,
                         "AGENT_CH_RLS_TEST_ADDR": f"127.0.0.1:{args.native_port}",
                         "AGENT_CH_RLS_TEST_READER_PASSWORD": pw["reader"],
                         "AGENT_CH_RLS_TEST_WRITER_PASSWORD": pw["writer"]},
                ).returncode
                if rc:
                    failures.append("live Rust tests: " + ", ".join(live))
        except (HarnessError, ch_creds.CredsError) as e:
            print(f"ch-integration: FAIL(harness): {e}", file=sys.stderr)
            return 1
        finally:
            ct.remove()
    if failures:
        print(f"ch-integration: CONTRACT failures: {', '.join(failures)}", file=sys.stderr)
        return 2
    print("PASS: ch-integration — credentials + row-level security hold.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
