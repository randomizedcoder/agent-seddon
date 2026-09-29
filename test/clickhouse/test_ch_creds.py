"""Tests for ch_creds.py and the pure parts of rls_harness.py: four-class tables
(positive_/negative_/corner_/boundary_ plus adversarial_ for the file contents and
roles that are untrusted input), and check-the-checks for the harness's row matcher —
a matcher that always passes would make `nix run .#ch-integration` meaningless.

Run: python3 -m unittest test_ch_creds -v
"""

from __future__ import annotations

import io
import json
import os
import re
import stat
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

import ch_creds as C
import rls_harness as H


def quiet(fn, *a, **kw):
    with redirect_stdout(io.StringIO()) as out, redirect_stderr(io.StringIO()) as err:
        rc = fn(*a, **kw)
    return rc, out.getvalue(), err.getvalue()


def mode(path: Path) -> int:
    return stat.S_IMODE(path.stat().st_mode)


class ValidatePassword(unittest.TestCase):
    CASES = [
        ("positive_generated_hex", "a1" * 24, True),
        ("positive_base64ish", "Abc.def_ghi~jkl+mn/o=p-q", True),
        ("boundary_16_chars", "a" * 16, True),
        ("boundary_15_chars", "a" * 15, False),
        ("boundary_256_chars", "a" * 256, True),
        ("boundary_257_chars", "a" * 257, False),
        ("negative_empty", "", False),
        ("corner_inner_space", "a" * 10 + " " + "a" * 10, False),
        ("adversarial_quote_breaks_sql", "a" * 16 + "'; DROP USER x; --", False),
        ("adversarial_newline_header_split", "a" * 16 + "\nX-Evil: 1", False),
        ("adversarial_colon_breaks_basic_auth", "a" * 16 + ":b", False),
        ("adversarial_xml_markup", "a" * 16 + "</password>", False),
        ("adversarial_non_ascii", "ä" * 20, False),
    ]

    def test_table(self):
        for name, value, ok in self.CASES:
            with self.subTest(name):
                if ok:
                    self.assertEqual(C.validate_password("reader", value), value)
                else:
                    with self.assertRaises(C.CredsError) as cm:
                        C.validate_password("reader", value)
                    # Never echo the rejected value.
                    if value:
                        self.assertNotIn(value, str(cm.exception))


class DefaultDir(unittest.TestCase):
    CASES = [
        ("positive_explicit_override", {"AGENT_CLICKHOUSE_SECRETS": "/s", "XDG_STATE_HOME": "/x"}, "/s"),
        ("positive_xdg_state", {"XDG_STATE_HOME": "/x", "HOME": "/h"}, "/x/agent-seddon/clickhouse"),
        ("negative_no_xdg_falls_back_to_home", {"HOME": "/h"}, "/h/.local/state/agent-seddon/clickhouse"),
        ("corner_blank_override_ignored", {"AGENT_CLICKHOUSE_SECRETS": "  ", "HOME": "/h"},
         "/h/.local/state/agent-seddon/clickhouse"),
    ]

    def test_table(self):
        for name, env, want in self.CASES:
            with self.subTest(name):
                self.assertEqual(C.default_dir(env), Path(want))


class PasswordPath(unittest.TestCase):
    def test_roles(self):
        root = Path("/r")
        for role in C.ROLES:
            with self.subTest(f"positive_{role}"):
                self.assertEqual(C.password_path(root, role), root / f"{role}.password")
        for name, role in [
            ("negative_unknown_role", "operator"),
            ("adversarial_traversal_role", "../admin"),
            ("adversarial_separator_role", "a/b"),
            ("corner_empty_role", ""),
        ]:
            with self.subTest(name), self.assertRaises(C.CredsError):
                C.password_path(root, role)


class Ensure(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name) / "creds"

    def tearDown(self):
        self.tmp.cleanup()

    def test_positive_fresh_dir_generates_every_role_private(self):
        created = C.ensure(self.root)
        self.assertEqual(created, list(C.ROLES))
        self.assertEqual(mode(self.root), 0o700)
        for role in C.ROLES:
            p = C.password_path(self.root, role)
            self.assertEqual(mode(p), 0o600, role)
            self.assertRegex(C.read_password(self.root, role), r"^[0-9a-f]{48}$")
        xml = C.server_xml_path(self.root)
        self.assertEqual(mode(xml), 0o644)
        text = xml.read_text()
        self.assertIn(C.sha256_hex(C.read_password(self.root, "admin")), text)
        # The rendered override holds the hash, never the plaintext.
        self.assertNotIn(C.read_password(self.root, "admin"), text)

    def test_positive_passwords_are_distinct(self):
        C.ensure(self.root)
        values = {C.read_password(self.root, r) for r in C.ROLES}
        self.assertEqual(len(values), len(C.ROLES))

    def test_corner_rerun_keeps_existing_passwords(self):
        C.ensure(self.root)
        before = {r: C.read_password(self.root, r) for r in C.ROLES}
        xml = C.server_xml_path(self.root)
        inode = xml.stat().st_ino
        self.assertEqual(C.ensure(self.root), [])
        self.assertEqual({r: C.read_password(self.root, r) for r in C.ROLES}, before)
        # Unchanged override is not rewritten (the container has the inode mounted).
        self.assertEqual(xml.stat().st_ino, inode)

    def test_corner_only_missing_role_generated(self):
        C.ensure(self.root)
        C.password_path(self.root, "viewer").unlink()
        self.assertEqual(C.ensure(self.root), ["viewer"])

    def test_positive_operator_supplied_password_kept(self):
        self.root.mkdir(mode=0o700)
        p = C.password_path(self.root, "admin")
        p.write_text("Operator.Chosen-Password1\n")
        p.chmod(0o600)
        C.ensure(self.root)
        self.assertEqual(C.read_password(self.root, "admin"), "Operator.Chosen-Password1")

    def test_negative_group_readable_dir_refused(self):
        self.root.mkdir(mode=0o750)
        self.root.chmod(0o750)
        with self.assertRaises(C.CredsError):
            C.ensure(self.root)

    def test_negative_world_readable_password_refused(self):
        C.ensure(self.root)
        C.password_path(self.root, "reader").chmod(0o644)
        with self.assertRaises(C.CredsError):
            C.ensure(self.root)

    def test_negative_root_is_a_file(self):
        self.root.write_text("x")
        with self.assertRaises(C.CredsError):
            C.ensure(self.root)

    def test_adversarial_malformed_password_file_refused_without_echo(self):
        C.ensure(self.root)
        p = C.password_path(self.root, "writer")
        p.write_text("short'; DROP\n")
        with self.assertRaises(C.CredsError) as cm:
            C.ensure(self.root)
        self.assertNotIn("DROP", str(cm.exception))

    def test_adversarial_symlinked_password_is_checked_like_a_file(self):
        # A password file that is a symlink to a world-readable file is refused: the
        # mode check follows the link to what would actually be read.
        C.ensure(self.root)
        outside = Path(self.tmp.name) / "shared"
        outside.write_text("a" * 32 + "\n")
        outside.chmod(0o644)
        p = C.password_path(self.root, "reader")
        p.unlink()
        p.symlink_to(outside)
        with self.assertRaises(C.CredsError):
            C.read_password(self.root, "reader")

    def test_negative_read_missing_password(self):
        with self.assertRaises(C.CredsError):
            C.read_password(self.root, "admin")


class AlterSql(unittest.TestCase):
    def test_positive_one_hashed_statement_per_sql_user(self):
        pw = {"writer": "w" * 20, "reader": "r" * 20, "viewer": "v" * 20}
        sql = C.alter_sql(pw)
        lines = sql.strip().splitlines()
        self.assertEqual(len(lines), 3)
        for role, user in C.SQL_USERS.items():
            line = next(l for l in lines if l.startswith(f"ALTER USER {user} "))
            self.assertIn(f"BY '{C.sha256_hex(pw[role])}' HOST ANY;", line)
            self.assertNotIn(pw[role], line)

    def test_negative_admin_is_not_altered(self):
        # `default` is an XML user; its password rides the override, not SQL.
        sql = C.alter_sql({"writer": "w" * 20, "reader": "r" * 20, "viewer": "v" * 20})
        self.assertNotIn("default", sql)

    def test_boundary_hash_is_hex_only(self):
        sql = C.alter_sql({"writer": "w" * 256, "reader": "r" * 16, "viewer": "v" * 16})
        for literal in re.findall(r"'([^']*)'", sql):
            self.assertRegex(literal, r"^[0-9a-f]{64}$")


class HyperdxConnections(unittest.TestCase):
    def test_positive_viewer_login(self):
        got = json.loads(C.hyperdx_connections("http://127.0.0.1:8123", "v" * 20))
        self.assertEqual(got[0]["username"], "agent_viewer")
        self.assertEqual(got[0]["password"], "v" * 20)
        self.assertEqual(got[0]["host"], "http://127.0.0.1:8123")

    def test_corner_json_escaping(self):
        # The password charset carries nothing JSON needs to escape, but the encoder
        # is what guarantees it.
        got = json.loads(C.hyperdx_connections('http://h/"x', "a/b+c=d~e.f_g-h1"))
        self.assertEqual(got[0]["host"], 'http://h/"x')


class Cli(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name) / "c"

    def tearDown(self):
        self.tmp.cleanup()

    def test_positive_ensure_then_path_then_alter(self):
        rc, _, err = quiet(C.main, ["--dir", str(self.root), "ensure"], {})
        self.assertEqual(rc, 0)
        self.assertIn("generated", err)
        rc, out, _ = quiet(C.main, ["--dir", str(self.root), "path", "reader"], {})
        self.assertEqual(out.strip(), str(self.root / "reader.password"))
        rc, out, _ = quiet(C.main, ["--dir", str(self.root), "path", "server-xml"], {})
        self.assertTrue(out.strip().endswith(C.SERVER_XML))
        rc, out, _ = quiet(C.main, ["--dir", str(self.root), "alter-sql"], {})
        self.assertEqual(rc, 0)
        for role in C.SQL_USERS:
            self.assertNotIn(C.read_password(self.root, role), out)

    def test_negative_alter_before_ensure_is_exit_2(self):
        rc, out, err = quiet(C.main, ["--dir", str(self.root), "alter-sql"], {})
        self.assertEqual(rc, 2)
        self.assertEqual(out, "")
        self.assertIn("ensure", err)

    def test_adversarial_path_unknown_role_is_exit_2(self):
        rc, _, _ = quiet(C.main, ["--dir", str(self.root), "path", "../../etc/shadow"], {})
        self.assertEqual(rc, 2)

    def test_corner_env_selects_dir(self):
        rc, out, _ = quiet(C.main, ["path", "admin"], {"AGENT_CLICKHOUSE_SECRETS": str(self.root)})
        self.assertEqual(out.strip(), str(self.root / "admin.password"))


def fake_query(replies):
    """A Query double returning `replies[(user, password)]`, else a refusal."""

    def run(user, password, sql):
        return replies.get((user, password), H.Reply(False, "Code: 516. auth failed"))

    return run


class HarnessCheck(unittest.TestCase):
    """check-the-checks: the row matcher must FAIL every way a row can be wrong."""

    PW = {"reader": "r" * 20, "writer": "w" * 20}

    def case(self, rows, role="reader"):
        return H.Case("c", "agent_reader", role, "SELECT 1", rows)

    def test_positive_expected_rows_match(self):
        q = fake_query({("agent_reader", "r" * 20): H.Reply(True, "rls-b\nrls-a\n")})
        self.assertIsNone(H.check(self.case(("rls-a", "rls-b")), q, self.PW))

    def test_positive_expected_refusal_matches(self):
        q = fake_query({})
        self.assertIsNone(H.check(self.case(None), q, self.PW))

    def test_negative_extra_tenant_row_fails(self):
        q = fake_query({("agent_reader", "r" * 20): H.Reply(True, "rls-a\nrls-b\n")})
        self.assertIsNotNone(H.check(self.case(("rls-a",)), q, self.PW))

    def test_negative_missing_row_fails(self):
        q = fake_query({("agent_reader", "r" * 20): H.Reply(True, "")})
        self.assertIsNotNone(H.check(self.case(("rls-a",)), q, self.PW))

    def test_negative_unexpected_success_fails(self):
        # The most important one: a login that should be refused but succeeds.
        q = fake_query({("agent_reader", None): H.Reply(True, "1\n")})
        self.assertIsNotNone(H.check(H.Case("c", "agent_reader", None, "SELECT 1", None), q, self.PW))

    def test_negative_error_when_rows_expected_fails(self):
        q = fake_query({})
        self.assertIsNotNone(H.check(self.case(()), q, self.PW))

    def test_corner_empty_rows_expected_and_got(self):
        q = fake_query({("agent_reader", "r" * 20): H.Reply(True, "")})
        self.assertIsNone(H.check(self.case(()), q, self.PW))

    def test_boundary_blank_lines_ignored(self):
        q = fake_query({("agent_reader", "r" * 20): H.Reply(True, "\nrls-a\n\n")})
        self.assertIsNone(H.check(self.case(("rls-a",)), q, self.PW))

    def test_adversarial_wrong_password_is_presented(self):
        # The matcher presents the case's role password, not whichever one works.
        q = fake_query({("agent_reader", "w" * 20): H.Reply(True, "rls-a\n")})
        self.assertIsNotNone(H.check(self.case(("rls-a",)), q, self.PW))


class HarnessMatrix(unittest.TestCase):
    def test_every_class_is_covered(self):
        names = [c.name for c in H.PRE_ALTER + H.MATRIX]
        for prefix in ("positive_", "negative_", "corner_", "boundary_", "adversarial_"):
            with self.subTest(prefix):
                self.assertTrue(any(n.startswith(prefix) for n in names), prefix)
        self.assertEqual(len(names), len(set(names)), "case names are unique")

    def test_every_role_is_refused_without_a_password(self):
        refused = {c.user for c in H.MATRIX if c.role is None and c.rows is None}
        self.assertEqual(refused, {"default", "agent_writer", "agent_reader", "agent_viewer"})


class SchemaPolicies(unittest.TestCase):
    """Static checks on the tenant row policies in nix/clickhouse/schema.sql."""

    SCHEMA = Path(os.environ.get("CH_SCHEMA", Path(__file__).resolve().parents[2] / "nix/clickhouse/schema.sql"))
    POLICY = re.compile(r"CREATE ROW POLICY (.*?) (tenant_iso_\w+)\s+ON \S+\s+USING (.*?) TO agent_reader;")

    def policies(self):
        found = self.POLICY.findall(self.SCHEMA.read_text())
        self.assertGreaterEqual(len(found), 12, "tenant policies parsed")
        return found

    def test_adversarial_every_tenant_policy_excludes_the_empty_tenant(self):
        for _, name, using in self.policies():
            with self.subTest(name):
                col = using.split(" ", 1)[0]
                self.assertIn(f"{col} != ''", using)

    def test_positive_tenant_policies_replace_on_reapply(self):
        # IF NOT EXISTS would leave an old, looser policy in place on a live database.
        for mode_, name, _ in self.policies():
            with self.subTest(name):
                self.assertEqual(mode_, "OR REPLACE")


if __name__ == "__main__":
    unittest.main()
