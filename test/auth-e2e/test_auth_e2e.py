"""Tables for auth_e2e.py (positive_ / negative_ / corner_ / boundary_ /
adversarial_), check-the-checks against fake agents, and — when
AUTH_E2E_AGENT is set — the live run against the real binary."""

from __future__ import annotations

import io
import json
import os
import subprocess
import sys
import tempfile
import time
import tomllib
import unittest
import urllib.error
import urllib.request
from pathlib import Path

import jwt

import auth_e2e as ae


def completed(returncode: int, stdout: str = "", stderr: str = "") -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess([], returncode, stdout, stderr)


def status(code: str) -> subprocess.CompletedProcess:
    return completed(1, "", f"ERROR:\n  Code: {code}\n  Message: nope\n")


class ParseReply(unittest.TestCase):
    def test_positive_ok_json(self):
        r = ae.parse_reply(0, '{"tenant": "a"}\n', "")
        self.assertEqual((r.code, r.body), ("OK", {"tenant": "a"}))

    def test_positive_status_line(self):
        self.assertEqual(ae.parse_reply(1, "", "ERROR:\n  Code: Unauthenticated\n  Message: x\n").code, "Unauthenticated")

    def test_corner_empty_reply_is_ok(self):
        self.assertEqual(ae.parse_reply(0, "\n", "").body, {})

    def test_corner_dial_failure(self):
        self.assertEqual(ae.parse_reply(1, "", "Failed to dial target host \"127.0.0.1:1\": x").code, "DIAL")

    def test_negative_unknown_failure_is_harness(self):
        with self.assertRaises(ae.HarnessError):
            ae.parse_reply(1, "", 'target server does not expose service "x"')

    def test_adversarial_non_json_success_is_harness(self):
        for out in ("not json", "[1, 2]", '"str"'):
            with self.subTest(out=out), self.assertRaises(ae.HarnessError):
                ae.parse_reply(0, out, "")

    def test_adversarial_message_cannot_forge_code(self):
        # A server message that mentions a code mid-line is not the status line.
        r = ae.parse_reply(1, "", "ERROR:\n  Code: PermissionDenied\n  Message: say Code: OK\n")
        self.assertEqual(r.code, "PermissionDenied")


class ClientArgv(unittest.TestCase):
    def capture(self, **kw) -> list[str]:
        seen: list[list[str]] = []

        def runner(argv, **_):
            seen.append(argv)
            return completed(0, "{}")

        client = ae.Client("grpcurl", Path("/ca"), kw.pop("cert", None), kw.pop("key", None), runner)
        client.call("127.0.0.1:1", "a.B/C", kw.pop("data", None), **kw)
        return seen[0]

    def test_positive_bearer_and_headers(self):
        argv = self.capture(bearer="tok", headers={"x-agent-session-id": "s"})
        self.assertIn("authorization: Bearer tok", argv)
        self.assertIn("x-agent-session-id: s", argv)

    def test_negative_no_cert_without_key(self):
        argv = self.capture(cert=Path("/c"))
        self.assertNotIn("-cert", argv)

    def test_corner_client_cert(self):
        argv = self.capture(cert=Path("/c"), key=Path("/k"))
        self.assertEqual(argv[argv.index("-cert") + 1], "/c")

    def test_boundary_request_body_is_json(self):
        argv = self.capture(data={"text": 'a"b'})
        self.assertEqual(json.loads(argv[argv.index("-d") + 1]), {"text": 'a"b'})

    def test_adversarial_address_and_method_are_last(self):
        # The method comes after `-d`, so a hostile body can't become a flag.
        argv = self.capture(data={"x": "-plaintext"})
        self.assertEqual(argv[-2:], ["127.0.0.1:1", "a.B/C"])


class IssuerTables(unittest.TestCase):
    def setUp(self):
        self.iss = ae.Issuer()
        self.iss.start()
        self.addCleanup(self.iss.stop)

    def fetch(self, path: str) -> dict:
        with urllib.request.urlopen(self.iss.url + path, timeout=5) as r:
            return json.load(r)

    def test_positive_token_verifies_against_published_jwks(self):
        jwks = self.fetch("/jwks")
        token = self.iss.mint(ae.login_claims(self.iss.url, "alice", "t"))
        key = jwt.PyJWK(jwks["keys"][0])
        claims = jwt.decode(token, key.key, algorithms=["ES256"], audience=ae.LOGIN_AUDIENCE, issuer=self.iss.url)
        self.assertEqual((claims["sub"], claims["org"]), ("alice", "t"))

    def test_positive_discovery_points_at_jwks(self):
        self.assertEqual(self.fetch("/.well-known/openid-configuration")["jwks_uri"], self.iss.url + "/jwks")

    def test_negative_unknown_path_404(self):
        with self.assertRaises(urllib.error.HTTPError) as e:
            self.fetch("/token")
        self.assertEqual(e.exception.code, 404)

    def test_boundary_expired_claims(self):
        c = ae.login_claims("i", "s", "t", now=1000, ttl=60)
        self.assertEqual((c["exp"], c["nbf"]), (1060, 995))

    def test_adversarial_foreign_key_fails_verification(self):
        from cryptography.hazmat.primitives.asymmetric import ec

        token = self.iss.mint(ae.login_claims(self.iss.url, "m", "t"), key=ec.generate_private_key(ec.SECP256R1()))
        key = jwt.PyJWK(self.fetch("/jwks")["keys"][0])
        with self.assertRaises(jwt.InvalidSignatureError):
            jwt.decode(token, key.key, algorithms=["ES256"], audience=ae.LOGIN_AUDIENCE)


def layout(work: Path) -> ae.Layout:
    return ae.Layout(work, work / "pki", "http://127.0.0.1:9", 50001, 50002)


class RenderConfig(unittest.TestCase):
    def test_positive_a_is_grpc_client_of_b(self):
        cfg = tomllib.loads(ae.render_config(layout(Path("/w")), "a"))
        self.assertEqual(cfg["memory"]["backend"], "grpc")
        self.assertEqual(cfg["grpc"]["memory"]["endpoint"], "https://127.0.0.1:50002")
        self.assertEqual(cfg["grpc"]["tls"]["client_ca"], "/w/pki/ca/root.crt")

    def test_positive_b_stores_files(self):
        cfg = tomllib.loads(ae.render_config(layout(Path("/w")), "b"))
        self.assertEqual(cfg["memory"]["episodic_path"], "/w/b/episodic.jsonl")
        self.assertNotIn("memory", cfg["grpc"])

    def test_positive_both_verify_the_same_agent_tokens(self):
        a, b = (tomllib.loads(ae.render_config(layout(Path("/w")), s))["auth"] for s in "ab")
        self.assertEqual(a["token"]["signing_key"], b["token"]["signing_key"])
        self.assertEqual((a["mode"], a["token"]["audience"]), ("oidc", ae.TOKEN_AUDIENCE))

    def test_negative_unknown_server(self):
        with self.assertRaises(ValueError):
            ae.render_config(layout(Path("/w")), "c")

    def test_corner_mtls_binding_names_fleet_in_tenant_a(self):
        cfg = tomllib.loads(ae.render_config(layout(Path("/w")), "a"))
        (binding,) = cfg["auth"]["mtls"]["bindings"]
        self.assertEqual((binding["san"], binding["tenant"]), ("spiffe://agent.e2e/svc/fleet", ae.TENANT_A))

    def test_adversarial_path_cannot_inject_keys(self):
        hostile = Path('/w"\nmode = "none"\n[x]\ny = "')
        cfg = tomllib.loads(ae.render_config(layout(hostile), "b"))
        self.assertEqual(cfg["auth"]["mode"], "oidc")
        self.assertNotIn("x", cfg)
        self.assertTrue(cfg["agent"]["working_dir"].startswith('/w"\nmode'))


class StoredIn(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())

    def write(self, rel: str, text: str) -> None:
        p = self.root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)

    def test_positive_tenant_partition(self):
        self.write("tenant-a/episodic.jsonl", "mk\n")
        self.assertEqual(ae.stored_in(self.root, "mk"), ["tenant-a"])

    def test_negative_absent(self):
        self.write("tenant-a/episodic.jsonl", "other\n")
        self.assertEqual(ae.stored_in(self.root, "mk"), [])

    def test_corner_unpartitioned_root(self):
        self.write("episodic.jsonl", "mk\n")
        self.assertEqual(ae.stored_in(self.root, "mk"), ["."])

    def test_boundary_missing_root(self):
        self.assertEqual(ae.stored_in(self.root / "nope", "mk"), [])

    def test_adversarial_leak_to_both_tenants_is_reported(self):
        self.write("tenant-a/episodic.jsonl", "mk\n")
        self.write("tenant-b/x/episodic.jsonl", "mk\n")
        self.assertEqual(ae.stored_in(self.root, "mk"), ["tenant-a", "tenant-b"])


class FakeAgent:
    """A grpcurl stand-in answering from a table: `method -> code or body`, with
    a default. Used to prove each step FAILS against a wrong agent."""

    def __init__(self, answers: dict, default="OK"):
        self.answers, self.default = answers, default

    def __call__(self, argv, **_):
        answer = self.answers.get(argv[-1], self.default)
        if isinstance(answer, dict):
            return completed(0, json.dumps(answer))
        if answer == "OK":
            return completed(0, "{}")
        return status(answer)


def fake_ctx(runner, work: Path) -> ae.Ctx:
    iss = ae.Issuer()
    client = ae.Client("grpcurl", Path("/ca"), Path("/c"), Path("/k"), runner)
    return ae.Ctx(layout(work), iss, client, client, client)


class CheckTheChecks(unittest.TestCase):
    """Each step must go red against an agent that gets it wrong."""

    def fails(self, step, runner, prep=None) -> str:
        work = Path(tempfile.mkdtemp())
        ctx = fake_ctx(runner, work)
        if prep:
            prep(ctx)
        out = io.StringIO()
        code = ae.run_steps(ctx, [("step", step)], out=out)
        self.assertEqual(code, ae.EXIT_CONTRACT, out.getvalue())
        return out.getvalue()

    def test_negative_open_agent_fails_no_bearer(self):
        self.fails(ae.step_no_bearer_refused, FakeAgent({}))

    def test_negative_accepting_bad_logins_fails(self):
        self.fails(ae.step_login_tokens_verified, FakeAgent({}, default={"accessToken": "t"}))

    def test_negative_exchange_without_handle_fails(self):
        self.fails(ae.step_exchange_mints_agent_tokens, FakeAgent({}, default={"accessToken": "x.y.z"}))

    def test_adversarial_header_tenant_winning_fails(self):
        def prep(ctx):
            ctx.tokens.update({n: {"access": "t"} for n in ("alice", "bob")})

        who = {"agent.v1.AuthService/WhoAmI": {"tenant": "someone-else", "sid": "s"}}
        self.fails(ae.step_whoami_tenant_beats_header, FakeAgent(who), prep)

    def test_adversarial_login_token_at_seam_fails(self):
        def prep(ctx):
            ctx.tokens["alice"] = {"login": "t"}

        self.fails(ae.step_login_token_refused_at_seams, FakeAgent({}), prep)

    def test_adversarial_unforwarded_chain_fails(self):
        # Append "succeeds" but nothing lands in B's tenant-A partition.
        def prep(ctx):
            ctx.tokens.update({n: {"access": "t"} for n in ("alice", "bob")})

        msg = self.fails(ae.step_chain_forwards_tenant, FakeAgent({}), prep)
        self.assertIn("landed in B's []", msg)

    def test_negative_hops_ignored_fails(self):
        def prep(ctx):
            ctx.tokens["alice"] = {"access": "t"}

        self.fails(ae.step_hops_ceiling, FakeAgent({}), prep)

    def test_negative_unbound_cert_accepted_fails(self):
        exch = {"agent.v1.AuthService/Exchange": {"accessToken": "t"}}
        who = {"agent.v1.AuthService/WhoAmI": {"tenant": ae.TENANT_A, "subject": "svc:fleet"}}
        self.fails(ae.step_mtls_service_identity, FakeAgent({**exch, **who}))

    def test_negative_refresh_replay_accepted_fails(self):
        def prep(ctx):
            ctx.tokens.update({n: {"access": "t", "refresh": "h1"} for n in ("alice", "bob")})

        rot = {"agent.v1.AuthService/Refresh": {"accessToken": "t2", "refreshHandle": "h2"}}
        self.fails(ae.step_refresh_and_logout, FakeAgent(rot), prep)

    def test_positive_run_stops_at_first_failure(self):
        ran = []
        steps = [("a", lambda c: ran.append("a")), ("b", self._boom), ("c", lambda c: ran.append("c"))]
        out = io.StringIO()
        self.assertEqual(ae.run_steps(None, steps, out=out), ae.EXIT_CONTRACT)
        self.assertEqual(ran, ["a"])
        self.assertIn("FAIL b", out.getvalue())

    @staticmethod
    def _boom(_ctx):
        raise ae.ContractError("boom")


@unittest.skipUnless(os.environ.get("AUTH_E2E_AGENT"), "set AUTH_E2E_AGENT to run the live chain")
class Live(unittest.TestCase):
    """The real thing: two agent processes, the fake issuer, the dev PKI."""

    def test_positive_live_chain(self):
        argv = [
            "--agent",
            os.environ["AUTH_E2E_AGENT"],
            "--grpcurl",
            os.environ["AUTH_E2E_GRPCURL"],
            "--pki-dev",
            os.environ["AUTH_E2E_PKI_DEV"],
        ]
        started = time.monotonic()
        self.assertEqual(ae.main(argv), ae.EXIT_OK)
        print(f"live chain: {time.monotonic() - started:.1f}s", file=sys.stderr)


if __name__ == "__main__":
    unittest.main()
