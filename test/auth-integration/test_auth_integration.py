"""Tables for auth_integration.py (positive_ / negative_ / corner_ / boundary_ /
adversarial_) and check-the-checks: every contract step is run against a fake
that behaves and against fakes that each break one promise, and must fail on
every broken one."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import tomllib
import unittest
import urllib.parse
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "auth-e2e"))
sys.path.insert(0, str(HERE.parent / "clickhouse"))

import auth_e2e as ae  # noqa: E402
import auth_integration as ai  # noqa: E402
import rls_harness  # noqa: E402


def completed(returncode: int, stdout: str = "", stderr: str = "") -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess([], returncode, stdout, stderr)


def positionals_after_flags(argv: list[str], n: int) -> bool:
    """The last `n` entries are positionals and no flag follows the first of them."""
    return not any(a.startswith("--") for a in argv[-n:])


# ---------------------------------------------------------------- pure builders


class CaArgv(unittest.TestCase):
    def test_positive_certificate_is_p256_with_every_san(self):
        argv = ai.ca_certificate_argv("step", "fleet", Path("/c"), Path("/k"), ("spiffe://x/svc/fleet", "localhost"),
                                      "https://127.0.0.1:1", Path("/root"), Path("/pw"))
        self.assertEqual(argv[argv.index("--kty") + 1], "EC")
        self.assertEqual(argv[argv.index("--curve") + 1], "P-256")
        sans = [argv[i + 1] for i, a in enumerate(argv) if a == "--san"]
        self.assertEqual(sans, ["spiffe://x/svc/fleet", "localhost"])
        self.assertEqual(argv[-3:], ["fleet", "/c", "/k"])

    def test_negative_password_value_never_in_argv(self):
        with tempfile.TemporaryDirectory() as d:
            pw = Path(d) / "pw"
            pw.write_text("s3cret-value")
            argvs = [ai.ca_init_argv("step", 1, pw),
                     ai.ca_certificate_argv("step", "a", Path("/c"), Path("/k"), (), "u", Path("/r"), pw)]
            for argv in argvs:
                self.assertNotIn("s3cret-value", " ".join(argv))
                self.assertIn(str(pw), argv)

    def test_corner_no_sans(self):
        argv = ai.ca_certificate_argv("step", "a", Path("/c"), Path("/k"), (), "u", Path("/r"), Path("/p"))
        self.assertNotIn("--san", argv)

    def test_boundary_renew_flags_precede_positionals(self):
        # `step ca renew` fails with "too many positional arguments" otherwise.
        argv = ai.ca_renew_argv("step", Path("/c"), Path("/k"), "https://127.0.0.1:1", Path("/r"))
        self.assertEqual(argv[-2:], ["/c", "/k"])
        self.assertTrue(positionals_after_flags(argv, 2))

    def test_boundary_init_binds_loopback_only(self):
        argv = ai.ca_init_argv("step", 19443, Path("/pw"))
        self.assertEqual(argv[argv.index("--address") + 1], "127.0.0.1:19443")

    def test_adversarial_san_cannot_become_a_flag_position(self):
        # A SAN is always the value of its own `--san`, and the positionals stay last.
        argv = ai.ca_certificate_argv("step", "a", Path("/c"), Path("/k"), ("--force",), "u", Path("/r"), Path("/p"))
        self.assertEqual(argv[argv.index("--san") + 1], "--force")
        self.assertEqual(argv[-3:], ["a", "/c", "/k"])


INSPECT = {
    "serial_number": "1234",
    "validity": {"start": "2026-09-27T00:00:00Z", "end": "2026-09-28T00:00:00Z"},
    "extensions": {"subject_alt_name": {"dns_names": ["localhost"],
                                        "uniform_resource_identifiers": ["spiffe://x/svc/fleet"]}},
}


class ParseInspect(unittest.TestCase):
    def test_positive_fields(self):
        info = ai.parse_inspect(json.dumps(INSPECT))
        self.assertEqual(info, ai.CertInfo("1234", "2026-09-28T00:00:00Z", ("spiffe://x/svc/fleet",)))

    def test_negative_missing_validity(self):
        with self.assertRaises(ae.HarnessError):
            ai.parse_inspect(json.dumps({"serial_number": "1"}))

    def test_corner_no_extensions_means_no_uris(self):
        d = {k: v for k, v in INSPECT.items() if k != "extensions"}
        self.assertEqual(ai.parse_inspect(json.dumps(d)).uris, ())

    def test_boundary_huge_serial_kept_exact(self):
        d = dict(INSPECT, serial_number=2**159 + 1)
        self.assertEqual(ai.parse_inspect(json.dumps(d)).serial, str(2**159 + 1))

    def test_adversarial_not_an_object(self):
        for text in ("not json", "[]", '"x"', "null"):
            with self.subTest(text=text), self.assertRaises(ae.HarnessError):
                ai.parse_inspect(text)


class PgDsn(unittest.TestCase):
    def test_positive_shape(self):
        self.assertEqual(ai.pg_dsn("abc", 15433), "postgres://agent:abc@127.0.0.1:15433/agent_auth")

    def test_adversarial_password_metacharacters_round_trip(self):
        pw = "p@ss:w/o?r#d%&=+ "
        dsn = ai.pg_dsn(pw, 1)
        parts = urllib.parse.urlsplit(dsn)
        self.assertEqual((parts.hostname, parts.port, parts.path), ("127.0.0.1", 1, "/agent_auth"))
        self.assertEqual(urllib.parse.unquote(parts.password), pw)

    def test_boundary_generated_password_is_url_safe(self):
        pg = ai.Postgres("podman", "n", "img", 1)
        self.assertIn(f":{pg.password}@", pg.dsn)


class ParseCounts(unittest.TestCase):
    def test_positive_rows(self):
        self.assertEqual(ai.parse_counts("tenant-a|2\ntenant-b|1\n"), {"tenant-a": 2, "tenant-b": 1})

    def test_corner_blank_lines_and_empty(self):
        self.assertEqual(ai.parse_counts("\n\n"), {})

    def test_boundary_zero(self):
        self.assertEqual(ai.parse_counts("t|0"), {"t": 0})

    def test_negative_missing_separator(self):
        with self.assertRaises(ae.HarnessError):
            ai.parse_counts("tenant-a 2")

    def test_adversarial_pipe_in_tenant_and_bad_count(self):
        self.assertEqual(ai.parse_counts("a|b|3"), {"a|b": 3})
        for row in ("a|", "a|-1", "a|1e9", "a|x"):
            with self.subTest(row=row), self.assertRaises(ae.HarnessError):
                ai.parse_counts(row)


class ParsePairs(unittest.TestCase):
    def test_positive_pairs(self):
        self.assertEqual(ai.parse_pairs("a\tlogin\nb\trefresh\n"), {("a", "login"), ("b", "refresh")})

    def test_corner_empty_tenant_for_verify_fail(self):
        self.assertEqual(ai.parse_pairs("\tverify_fail\n"), {("", "verify_fail")})

    def test_boundary_no_rows(self):
        self.assertEqual(ai.parse_pairs(""), set())

    def test_negative_wrong_width(self):
        for body in ("a\tb\tc", "just-one"):
            with self.subTest(body=body), self.assertRaises(ae.HarnessError):
                ai.parse_pairs(body)


class Plan(unittest.TestCase):
    def names(self, steps) -> list[str]:
        return [n for n, _ in steps]

    def test_positive_all_tiers(self):
        steps = ai.plan(True, True)
        self.assertEqual(self.names(steps),
                         self.names(ai.STEPCA_STEPS + ai.PG_STEPS + ai.CH_STEPS))

    def test_negative_clickhouse_without_postgres(self):
        with self.assertRaises(ValueError):
            ai.plan(False, True)

    def test_corner_no_containers_uses_file_sessions(self):
        self.assertEqual(self.names(ai.plan(False, False)), self.names(ai.STEPCA_STEPS + ai.FILE_SESSION_STEPS))

    def test_boundary_step_ca_tier_always_first(self):
        for pg, ch in ((True, True), (True, False), (False, False)):
            with self.subTest(pg=pg, ch=ch):
                self.assertEqual(ai.plan(pg, ch)[: len(ai.STEPCA_STEPS)], ai.STEPCA_STEPS)


class ConfigSections(unittest.TestCase):
    def parse(self, lines) -> dict:
        return tomllib.loads("\n".join(lines))

    def test_positive_both_tiers(self):
        doc = self.parse(ai.extra_sections(pg=True, ch_native=19001, writer_password_file=Path("/w")))
        self.assertEqual(doc["config_store"], {"dsn_ref": f"env:{ai.PG_DSN_ENV}", "migrate_on_start": True})
        self.assertEqual(doc["telemetry"]["clickhouse_url"], "127.0.0.1:19001")
        self.assertEqual(doc["telemetry"]["user"], "agent_writer")

    def test_negative_clickhouse_needs_password_file(self):
        with self.assertRaises(ValueError):
            ai.extra_sections(pg=False, ch_native=1, writer_password_file=None)

    def test_corner_no_tiers_adds_nothing(self):
        self.assertEqual(ai.extra_sections(pg=False, ch_native=None, writer_password_file=None), [])

    def test_boundary_session_lines(self):
        self.assertEqual(ai.session_lines(True, Path("/s")), ('session_store = "postgres"',))
        doc = tomllib.loads("\n".join(ai.session_lines(False, Path("/s"))))
        self.assertEqual(doc, {"session_store": "file", "session_path": "/s/auth-sessions"})

    def test_adversarial_dsn_never_rendered_and_paths_stay_strings(self):
        hostile = Path('/tmp/a"b\nc = 1')
        lines = ai.extra_sections(pg=True, ch_native=1, writer_password_file=hostile)
        doc = self.parse(lines)
        self.assertEqual(doc["telemetry"]["password_file"], str(hostile))
        self.assertNotIn("postgres://", "\n".join(lines))


# ---------------------------------------------------------------- process wrappers


class Recorder:
    def __init__(self, replies=None):
        self.calls: list[tuple[list[str], dict]] = []
        self.replies = list(replies or [])

    def __call__(self, argv, **kw):
        self.calls.append((list(argv), kw))
        return self.replies.pop(0) if self.replies else completed(0, "")


class PostgresWrapper(unittest.TestCase):
    def test_positive_password_in_env_not_argv(self):
        rec = Recorder()
        pg = ai.Postgres("podman", "n", "img", 1, run=rec)
        pg.start()
        for argv, kw in rec.calls:
            self.assertNotIn(pg.password, " ".join(argv))
            self.assertEqual(kw["env"]["POSTGRES_PASSWORD"], pg.password)
        run = next(argv for argv, _ in rec.calls if argv[1] == "run")
        self.assertEqual(run[run.index("-p") + 1], "127.0.0.1:1:5432")

    def test_negative_run_failure_is_harness(self):
        rec = Recorder([completed(0), completed(125, stderr="no image")])
        with self.assertRaises(ae.HarnessError):
            ai.Postgres("podman", "n", "img", 1, run=rec).start()

    def test_corner_waits_for_a_real_query(self):
        rec = Recorder([completed(0), completed(0), completed(2), completed(2), completed(0, "1\n")])
        with mock.patch.object(ai.time, "sleep"):
            ai.Postgres("podman", "n", "img", 1, run=rec).start()
        self.assertEqual(len(rec.calls), 5)

    def test_boundary_sql_goes_over_stdin(self):
        rec = Recorder([completed(0, "t|1\n")])
        out = ai.Postgres("podman", "n", "img", 1, run=rec).sql("SELECT 1;")
        self.assertEqual(out, "t|1\n")
        self.assertEqual(rec.calls[0][1]["input"], "SELECT 1;")
        self.assertNotIn("SELECT 1;", rec.calls[0][0])

    def test_adversarial_query_error_is_harness(self):
        rec = Recorder([completed(3, stderr="ERROR: relation \"cards\" does not exist")])
        with self.assertRaises(ae.HarnessError):
            ai.Postgres("podman", "n", "img", 1, run=rec).sql("SELECT * FROM cards;")


class StepCaWrapper(unittest.TestCase):
    def test_positive_private_steppath(self):
        with tempfile.TemporaryDirectory() as d:
            rec = Recorder()
            ca = ai.StepCa("step", "step-ca", Path(d), 1, run=rec)
            ca.init()
            self.assertEqual(rec.calls[0][1]["env"]["STEPPATH"], str(Path(d) / "steppath"))
            self.assertEqual(ca.password_file.stat().st_mode & 0o777, 0o600)

    def test_negative_step_failure_is_harness(self):
        with tempfile.TemporaryDirectory() as d:
            ca = ai.StepCa("step", "step-ca", Path(d), 1, run=Recorder([completed(1, stderr="denied")]))
            with self.assertRaises(ae.HarnessError):
                ca.renew(Path("/c"), Path("/k"))

    def test_corner_inspect_parses_step_output(self):
        with tempfile.TemporaryDirectory() as d:
            ca = ai.StepCa("step", "step-ca", Path(d), 1, run=Recorder([completed(0, json.dumps(INSPECT))]))
            self.assertEqual(ca.inspect(Path("/c")).serial, "1234")

    def test_boundary_urls_are_loopback(self):
        ca = ai.StepCa("step", "step-ca", Path("/w"), 19443)
        self.assertEqual(ca.url, "https://127.0.0.1:19443")
        self.assertEqual(ca.root, Path("/w/steppath/certs/root_ca.crt"))


# ---------------------------------------------------------------- check-the-checks


class FakeCa:
    """The daemon: `renew` mints a new serial; the broken variants keep it or
    drop the SPIFFE name."""

    def __init__(self, bug: str = ""):
        self.gen, self.bug = 1, bug

    def inspect(self, _crt) -> ai.CertInfo:
        serial = "1" if self.bug == "same_serial" else str(self.gen)
        uris = () if (self.bug == "lost_spiffe" and self.gen > 1) else (ae.spiffe("fleet"),)
        end = "2026-09-27T00:00:00Z" if (self.bug == "shorter" and self.gen > 1) else f"2026-09-2{7 + self.gen}T00:00:00Z"
        return ai.CertInfo(serial, end, uris)

    def renew(self, _crt, _key) -> None:
        self.gen += 1


class FakeAgentA:
    """Agent A as the steps see it, over grpcurl argv. `bug` breaks one promise."""

    def __init__(self, ca: FakeCa | None = None, bug: str = ""):
        self.ca, self.bug = ca, bug
        self.handles: dict[str, str] = {}  # live refresh handle → session
        self.spent: dict[str, str] = {}  # rotated-out handle → session
        self.revoked: set[str] = set()
        self.access: dict[str, str] = {}  # access token → session
        self.n = 0
        self.kids = ["k1", "k0"]  # the published key set, current first

    def seed(self, who: str) -> dict:
        self.n += 1
        tok = {"access": f"acc-{who}-{self.n}", "refresh": f"ref-{who}-{self.n}"}
        self.handles[tok["refresh"]] = who
        self.access[tok["access"]] = who
        return tok

    def restart(self) -> None:
        if self.bug == "forget_on_restart":
            self.handles.clear()

    def reply(self, method: str, data: dict, bearer: str | None, cert: str | None) -> tuple[str, dict]:
        foreign = cert is not None and "foreign" in cert
        if foreign and self.bug != "accept_foreign":
            return "DIAL", {}
        if method.endswith("Health/Check"):
            return "OK", {"status": "SERVING"}
        if method == ai.JWKS:
            return "OK", {"jwksJson": json.dumps({"keys": [{"kid": k} for k in self.kids]})}
        if method == ai.EXCHANGE and data.get("useClientCert"):
            return "OK", {"accessToken": f"svc-{self.ca.gen if self.ca else 0}"}
        if method == ai.WHOAMI:
            if bearer and bearer.startswith("svc-"):
                # A bound token is honoured over its own certificate or any bound
                # service's (the fleet leaf here), never elsewhere.
                bound_peer = cert is not None and "/fleet/" in cert
                if self.bug == "strand_on_rotation":
                    bound_peer = bound_peer and bearer == f"svc-{self.ca.gen}"
                if bound_peer or self.bug == "unbound_token":
                    return "OK", {"tenant": ae.TENANT_A, "subject": "svc:fleet"}
            return "Unauthenticated", {}
        if method == ai.REFRESH:
            h = data["refreshHandle"]
            if h in self.handles and self.handles[h] not in self.revoked:
                who = self.handles.pop(h)
                if self.bug == "no_rotation":
                    self.handles[h] = who
                    return "OK", {"refreshHandle": h}
                self.spent[h] = who
                new = f"{h}+"
                self.handles[new] = who
                return "OK", {"refreshHandle": new}
            if h in self.spent:
                if self.bug == "replay_ok":
                    return "OK", {"refreshHandle": f"{h}!"}
                self.revoked.add(self.spent[h])
            return "Unauthenticated", {}
        if method == ai.LOGOUT:
            who = self.access.get(bearer or "")
            if who and self.bug != "logout_noop":
                self.revoked.add(who)
            return ("OK", {}) if who else ("Unauthenticated", {})
        return "Unimplemented", {}

    def runner(self, argv, **_):
        bearer = cert = None
        for i, a in enumerate(argv):
            if a == "-H" and argv[i + 1].startswith("authorization: Bearer "):
                bearer = argv[i + 1].removeprefix("authorization: Bearer ")
            if a == "-cert":
                cert = argv[i + 1]
        data = json.loads(argv[argv.index("-d") + 1])
        code, body = self.reply(argv[-1], data, bearer, cert)
        if code == "OK":
            return completed(0, json.dumps(body))
        if code == "DIAL":
            return completed(1, "", 'Failed to dial target host "x": tls: bad certificate')
        return completed(1, "", f"ERROR:\n  Code: {code}\n  Message: no\n")


def it_ctx(agent: FakeAgentA, **kw) -> ai.ItCtx:
    work = Path("/w")
    lay = ae.Layout(work, work / "pki", "http://issuer", 1, 2)
    client = lambda cert=None: ae.Client("grpcurl", lay.ca, Path(cert) if cert else None,
                                          Path("/k") if cert else None, agent.runner)
    ctx = ai.ItCtx(lay=lay, issuer=None, user=client("/pki/cli/cert.pem"), fleet=client("/pki/fleet/cert.pem"),
                   bare=client(), foreign=client("/foreign/fleet/cert.pem"), restart_a=agent.restart, **kw)
    ctx.tokens = {"alice": agent.seed("alice"), "bob": agent.seed("bob")}
    return ctx


class CheckRenewal(unittest.TestCase):
    def run_with(self, ca_bug="", agent_bug=""):
        ca = FakeCa(ca_bug)
        ai.step_renewal_keeps_identity(it_ctx(FakeAgentA(ca, agent_bug), ca=ca))

    def test_positive_good_daemon_passes(self):
        self.run_with()

    def test_negative_each_broken_promise_fails(self):
        for ca_bug, agent_bug in (("same_serial", ""), ("lost_spiffe", ""), ("shorter", ""),
                                  ("", "unbound_token"), ("", "strand_on_rotation")):
            with self.subTest(ca_bug=ca_bug, agent_bug=agent_bug), self.assertRaises(ae.ContractError):
                self.run_with(ca_bug, agent_bug)


class FakeReload:
    """Agent A's reload as the SIGHUP step sees it: the serial it serves, the key
    rotation on disk, the signal. `bug` breaks one promise."""

    def __init__(self, ca: FakeCa, agent: FakeAgentA, bug: str = ""):
        self.ca, self.agent, self.bug = ca, agent, bug
        self.served, self.pending, self.alive = ca.gen, None, True

    def served_serial(self) -> str:
        return str(self.ca.gen if self.bug == "live_before_signal" else self.served)

    def rotate(self) -> None:
        self.pending = ["k2", self.agent.kids[0]]
        if self.bug == "keys_before_signal":
            self.agent.kids = self.pending

    def hup(self) -> None:
        if self.bug == "dies":
            self.alive = False
            return
        if self.bug != "tls_not_reloaded":
            self.served = self.ca.gen
        if self.bug == "key_not_reloaded":
            return
        self.agent.kids = self.pending[:1] if self.bug == "previous_dropped" else self.pending


class CheckSighupReload(unittest.TestCase):
    def run_with(self, bug=""):
        ca = FakeCa()
        agent = FakeAgentA(ca)
        fake = FakeReload(ca, agent, bug)
        ctx = it_ctx(agent, ca=ca, served_serial=fake.served_serial, rotate_signer=fake.rotate,
                     hup_a=fake.hup, a_alive=lambda: fake.alive, reload_wait=0.3)
        ai.step_sighup_reloads_tls_and_signing_key(ctx)

    def test_positive_reload_on_signal_passes(self):
        self.run_with()

    def test_negative_each_broken_promise_fails(self):
        for bug in ("tls_not_reloaded", "key_not_reloaded", "previous_dropped", "dies",
                    "live_before_signal", "keys_before_signal"):
            with self.subTest(bug=bug), self.assertRaises(ae.ContractError):
                self.run_with(bug)


class WithPreviousKey(unittest.TestCase):
    BASE = "[auth.token]\nissuer = \"x\"\nsigning_key = \"/k/signer.pem\"\nttl_secs = 300"

    def test_positive_inserted_after_signing_key(self):
        got = ai.with_previous_key(self.BASE, Path("/k/prev.pem")).split("\n")
        i = got.index('signing_key = "/k/signer.pem"')
        self.assertEqual(got[i + 1], 'previous_key = "/k/prev.pem"')
        self.assertEqual(tomllib.loads(ai.with_previous_key(self.BASE, Path("/k/prev.pem")))["auth"]["token"]
                         ["previous_key"], "/k/prev.pem")

    def test_negative_no_signing_key(self):
        with self.assertRaises(ValueError):
            ai.with_previous_key("[auth.token]\nissuer = \"x\"", Path("/p"))

    def test_corner_already_has_previous_key(self):
        with self.assertRaises(ValueError):
            ai.with_previous_key(self.BASE + '\nprevious_key = "/old"', Path("/p"))

    def test_boundary_two_signing_keys_refused(self):
        with self.assertRaises(ValueError):
            ai.with_previous_key(self.BASE + '\nsigning_key = "/again"', Path("/p"))

    def test_adversarial_path_with_quotes_stays_one_toml_string(self):
        path = Path('/k/"]\nmode = "none')
        doc = tomllib.loads(ai.with_previous_key(self.BASE, path))
        self.assertEqual(doc["auth"]["token"]["previous_key"], str(path))
        self.assertNotIn("mode", doc["auth"]["token"])


class ParseKids(unittest.TestCase):
    def test_positive_in_order(self):
        self.assertEqual(ai.parse_kids('{"keys":[{"kid":"b"},{"kid":"a"}]}'), ["b", "a"])

    def test_corner_empty_set(self):
        self.assertEqual(ai.parse_kids('{"keys":[]}'), [])

    def test_negative_not_json(self):
        with self.assertRaises(ae.ContractError):
            ai.parse_kids("nope")

    def test_boundary_key_without_kid(self):
        with self.assertRaises(ae.ContractError):
            ai.parse_kids('{"keys":[{"kty":"EC"}]}')

    def test_adversarial_wrong_shape(self):
        for doc in ('{"keys":"k1"}', '[1,2]', '{"keys":null}'):
            with self.subTest(doc=doc), self.assertRaises(ae.ContractError):
                ai.parse_kids(doc)


class CheckForeignCa(unittest.TestCase):
    def test_positive_refused(self):
        ai.step_foreign_ca_refused(it_ctx(FakeAgentA()))

    def test_adversarial_foreign_accepted_fails(self):
        with self.assertRaises(ae.ContractError):
            ai.step_foreign_ca_refused(it_ctx(FakeAgentA(bug="accept_foreign")))


class CheckSessionsInPostgres(unittest.TestCase):
    def ctx(self, counts: str, blobs: list[bytes]) -> ai.ItCtx:
        def pg(sql: str) -> str:
            return counts if "count(*)" in sql else "\n".join(b.hex() for b in blobs)
        return it_ctx(FakeAgentA(), pg=pg)

    def test_positive_rows_per_tenant_no_clear_secrets(self):
        ctx = self.ctx(f"{ae.TENANT_A}|1\n{ae.TENANT_B}|1\n", [b'{"hash":"9f86d0"}', b'{"hash":"aa"}'])
        ai.step_sessions_in_postgres(ctx)

    def test_negative_missing_tenant_fails(self):
        with self.assertRaises(ae.ContractError):
            ai.step_sessions_in_postgres(self.ctx(f"{ae.TENANT_A}|2\n", []))

    def test_adversarial_clear_refresh_handle_fails(self):
        ctx = self.ctx(f"{ae.TENANT_A}|1\n{ae.TENANT_B}|1\n", [])
        leaked = ctx.tokens["bob"]["refresh"].encode()
        ctx.pg = lambda sql: (f"{ae.TENANT_A}|1\n{ae.TENANT_B}|1\n" if "count(*)" in sql
                              else (b"prefix" + leaked + b"suffix").hex())
        with self.assertRaises(ae.ContractError):
            ai.step_sessions_in_postgres(ctx)


class CheckSessionsSurviveRestart(unittest.TestCase):
    def test_positive_good_store_passes(self):
        ai.step_sessions_survive_restart(it_ctx(FakeAgentA()))

    def test_negative_each_broken_promise_fails(self):
        for bug in ("forget_on_restart", "no_rotation", "replay_ok", "logout_noop"):
            with self.subTest(bug=bug), self.assertRaises(ae.ContractError):
                ai.step_sessions_survive_restart(it_ctx(FakeAgentA(bug=bug)))


GOOD_EVENTS = "\n".join(f"{t}\t{e}" for t, e in sorted(ai.EXPECTED_EVENTS))


def ch_fake(*, events=GOOD_EVENTS, reader=None, unscoped=rls_harness.Reply(False, "Code: 164 readonly"),
            everything="ts\tuser\tlogin\n"):
    reader = reader or {ae.TENANT_A: ae.TENANT_A, ae.TENANT_B: ae.TENANT_B}

    def ch(role: str, sql: str) -> rls_harness.Reply:
        if role == "reader":
            for tenant, seen in reader.items():
                if f"SQL_tenant_id = '{tenant}'" in sql:
                    return rls_harness.Reply(True, seen + "\n")
            return unscoped
        if "DISTINCT user, event" in sql:
            return rls_harness.Reply(True, events)
        return rls_harness.Reply(True, everything)
    return ch


class CheckAudit(unittest.TestCase):
    def ctx(self, **kw) -> ai.ItCtx:
        return it_ctx(FakeAgentA(), ch=ch_fake(**kw), audit_wait=0)

    def test_positive_all_audit_steps_pass(self):
        ctx = self.ctx()
        for _, step in ai.CH_STEPS:
            step(ctx)

    def test_corner_unscoped_reader_zero_rows_passes(self):
        ai.step_audit_rows_tenant_scoped(self.ctx(unscoped=rls_harness.Reply(True, "0\n")))

    def test_negative_missing_event_fails(self):
        events = "\n".join(line for line in GOOD_EVENTS.splitlines() if "revoke" not in line)
        with self.assertRaises(ae.ContractError):
            ai.step_audit_rows_land(self.ctx(events=events))

    def test_negative_reader_sees_other_tenant_fails(self):
        both = f"{ae.TENANT_A}\n{ae.TENANT_B}"
        with self.assertRaises(ae.ContractError):
            ai.step_audit_rows_tenant_scoped(self.ctx(reader={ae.TENANT_A: both, ae.TENANT_B: ae.TENANT_B}))

    def test_boundary_unscoped_reader_reading_rows_fails(self):
        with self.assertRaises(ae.ContractError):
            ai.step_audit_rows_tenant_scoped(self.ctx(unscoped=rls_harness.Reply(True, "5\n")))

    def test_adversarial_token_material_in_rows_fails(self):
        ctx = self.ctx()
        for body in (f"x\t{ctx.tokens['alice']['refresh']}\n", "x\teyJhbGciOi\n", "x\tnot-a-token\n"):
            ctx.ch = ch_fake(everything=body)
            with self.subTest(body=body), self.assertRaises(ae.ContractError):
                ai.step_audit_rows_carry_no_secrets(ctx)

    def test_adversarial_writer_refused_is_contract(self):
        ctx = self.ctx()
        ctx.ch = lambda role, sql: rls_harness.Reply(False, "Code: 516 authentication failed")
        with self.assertRaises(ae.ContractError):
            ai.step_audit_rows_carry_no_secrets(ctx)


if __name__ == "__main__":
    unittest.main()
