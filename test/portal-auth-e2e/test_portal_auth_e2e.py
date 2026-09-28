"""Tables for portal_auth_e2e.py (positive_ / negative_ / corner_ / boundary_ /
adversarial_) and check-the-checks: every contract step runs against a fake
portal + agent + edge that behaves, and against fakes that each break one promise,
and must fail on every broken one."""

from __future__ import annotations

import io
import json
import secrets
import sys
import tomllib
import unittest
import urllib.parse
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "auth-e2e"))
sys.path.insert(0, str(HERE.parent / "portal-envoy"))

import auth_e2e as ae  # noqa: E402
import portal_auth_e2e as pa  # noqa: E402
import portal_envoy  # noqa: E402

SECRET = "client-s3cret"
PORTAL = "http://127.0.0.1:18092/"


def flow(**kw) -> pa.CodeFlow:
    return pa.CodeFlow(
        kw.get("client_id", pa.CLIENT_ID),
        kw.get("secret", SECRET),
        kw.get("redirect_uris", [PORTAL]),
        lambda claims: "idtoken." + json.dumps(claims, sort_keys=True),
        lambda nonce: {"sub": pa.USER, "nonce": nonce},
    )


def authorize_query(**over) -> str:
    q = {
        "response_type": "code",
        "client_id": pa.CLIENT_ID,
        "redirect_uri": PORTAL,
        "code_challenge": pa.s256("v" * 43),
        "code_challenge_method": "S256",
        "state": "st-1",
        "nonce": "n-1",
    }
    q.update(over)
    return urllib.parse.urlencode({k: v for k, v in q.items() if v is not None})


def code_of(answer: pa.Answer) -> str:
    return urllib.parse.parse_qs(urllib.parse.urlsplit(answer.location).query)["code"][0]


def redeem_form(code: str, **over) -> str:
    f = {
        "grant_type": "authorization_code",
        "client_id": pa.CLIENT_ID,
        "client_secret": SECRET,
        "code": code,
        "redirect_uri": PORTAL,
        "code_verifier": "v" * 43,
    }
    f.update(over)
    return urllib.parse.urlencode(f)


def frame(flag: int, payload: bytes) -> bytes:
    return bytes([flag]) + len(payload).to_bytes(4, "big") + payload


# ---------------------------------------------------------------- the fake IdP


class S256(unittest.TestCase):
    def test_positive_rfc7636_appendix_b(self):
        self.assertEqual(
            pa.s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
        )

    def test_boundary_no_padding(self):
        self.assertNotIn("=", pa.s256(""))
        self.assertEqual(len(pa.s256("x")), 43)


class WithQuery(unittest.TestCase):
    def test_positive_appends(self):
        self.assertEqual(pa.with_query(PORTAL, {"code": "c", "state": "s"}), PORTAL + "?code=c&state=s")

    def test_corner_keeps_an_existing_query(self):
        self.assertEqual(pa.with_query("http://h/p?a=1", {"b": "2"}), "http://h/p?a=1&b=2")

    def test_adversarial_values_are_encoded(self):
        url = pa.with_query(PORTAL, {"state": "x&code=evil#frag"})
        q = urllib.parse.parse_qs(urllib.parse.urlsplit(url).query)
        self.assertEqual(q, {"state": ["x&code=evil#frag"]})
        self.assertEqual(urllib.parse.urlsplit(url).fragment, "")


class SingleValues(unittest.TestCase):
    def test_positive(self):
        self.assertEqual(pa.single_values("a=1&b=2"), {"a": "1", "b": "2"})

    def test_corner_empty(self):
        self.assertEqual(pa.single_values(""), {})

    def test_boundary_blank_value_kept(self):
        self.assertEqual(pa.single_values("a="), {"a": ""})

    def test_adversarial_repeated_key_refused(self):
        self.assertIsNone(pa.single_values("code=a&code=b"))


class CodeFlowTable(unittest.TestCase):
    def test_positive_consent_then_redeem(self):
        f = flow()
        a = f.authorize(authorize_query())
        self.assertEqual(a.status, 302)
        q = urllib.parse.parse_qs(urllib.parse.urlsplit(a.location).query)
        self.assertEqual(q["state"], ["st-1"])
        r = f.redeem(redeem_form(code_of(a)))
        self.assertEqual(r.status, 200)
        self.assertIn('"nonce": "n-1"', r.body["id_token"])
        self.assertEqual([x[2] for x in f.authorizations], ["consented"])
        self.assertEqual([x[1] for x in f.redemptions], ["ok"])

    def test_negative_wrong_client_secret(self):
        f = flow()
        r = f.redeem(redeem_form(code_of(f.authorize(authorize_query())), client_secret="nope"))
        self.assertEqual((r.status, r.body), (400, {"error": "invalid_client"}))

    def test_negative_wrong_verifier(self):
        f = flow()
        r = f.redeem(redeem_form(code_of(f.authorize(authorize_query())), code_verifier="w" * 43))
        self.assertEqual(r.body, {"error": "invalid_grant"})

    def test_negative_plain_pkce_refused(self):
        a = flow().authorize(authorize_query(code_challenge_method="plain"))
        self.assertEqual((a.status, a.location), (400, ""))

    def test_negative_no_challenge_refused(self):
        self.assertEqual(flow().authorize(authorize_query(code_challenge=None)).status, 400)

    def test_corner_deny_next_answers_once(self):
        f = flow()
        f.deny_next = "access_denied"
        a = f.authorize(authorize_query())
        self.assertEqual(a.status, 302)
        q = urllib.parse.parse_qs(urllib.parse.urlsplit(a.location).query)
        self.assertEqual(q, {"error": ["access_denied"], "state": ["st-1"]})
        self.assertIn("code", f.authorize(authorize_query()).location)
        self.assertEqual([x[2] for x in f.authorizations], ["denied", "consented"])

    def test_boundary_redeem_redirect_uri_must_match_exactly(self):
        f = flow()
        r = f.redeem(redeem_form(code_of(f.authorize(authorize_query())), redirect_uri=PORTAL.rstrip("/")))
        self.assertEqual(r.body, {"error": "invalid_grant"})

    def test_adversarial_code_redeemed_once(self):
        f = flow()
        code = code_of(f.authorize(authorize_query()))
        self.assertEqual(f.redeem(redeem_form(code)).status, 200)
        self.assertEqual(f.redeem(redeem_form(code)).body, {"error": "invalid_grant"})

    def test_adversarial_unregistered_redirect_is_not_followed(self):
        f = flow()
        a = f.authorize(authorize_query(redirect_uri="http://evil.example/"))
        self.assertEqual((a.status, a.location), (400, ""))
        self.assertEqual(f.authorizations[-1][2], "invalid_redirect_uri")

    def test_adversarial_repeated_parameter_refused(self):
        f = flow()
        a = f.authorize(authorize_query() + "&redirect_uri=http%3A%2F%2Fevil.example%2F")
        self.assertEqual((a.status, a.location), (400, ""))

    def test_adversarial_unknown_code(self):
        self.assertEqual(flow().redeem(redeem_form("forged")).body, {"error": "invalid_grant"})


class CodeIssuerDiscovery(unittest.TestCase):
    def test_positive_code_endpoints_advertised(self):
        issuer = pa.CodeIssuer(SECRET, [PORTAL])
        try:
            d = issuer.discovery()
            self.assertEqual(d["authorization_endpoint"], issuer.url + "/authorize")
            self.assertEqual(d["token_endpoint"], issuer.url + "/token")
            self.assertEqual(d["jwks_uri"], issuer.url + "/jwks")
            self.assertEqual(d["code_challenge_methods_supported"], ["S256"])
        finally:
            issuer._server.server_close()

    def test_positive_minted_id_token_carries_nonce_and_audience(self):
        import jwt

        issuer = pa.CodeIssuer(SECRET, [PORTAL])
        try:
            a = issuer.flow.authorize(authorize_query())
            body = issuer.flow.redeem(redeem_form(code_of(a))).body
            claims = jwt.decode(body["id_token"], issuer.key.public_key(), algorithms=["ES256"], audience=pa.CLIENT_ID)
            self.assertEqual((claims["nonce"], claims["org"], claims["iss"]), ("n-1", ae.TENANT_A, issuer.url))
        finally:
            issuer._server.server_close()


# ---------------------------------------------------------------- grpc-web


class TrailerStatus(unittest.TestCase):
    def test_positive_after_a_data_frame(self):
        body = frame(0, b"\x0a\x00") + frame(0x80, b"grpc-status: 0\r\ngrpc-message: \r\n")
        self.assertEqual(pa.trailer_status(body), 0)

    def test_negative_no_trailer(self):
        self.assertIsNone(pa.trailer_status(frame(0, b"abc")))

    def test_corner_empty_body(self):
        self.assertIsNone(pa.trailer_status(b""))

    def test_boundary_status_sixteen_and_seventeen(self):
        self.assertEqual(pa.trailer_status(frame(0x80, b"grpc-status: 16\r\n")), 16)
        self.assertIsNone(pa.trailer_status(frame(0x80, b"grpc-status: 17\r\n")))

    def test_adversarial_length_past_the_body(self):
        self.assertIsNone(pa.trailer_status(b"\x80\x00\x00\x10\x00grpc-status: 0"))

    def test_adversarial_truncated_header(self):
        self.assertIsNone(pa.trailer_status(frame(0, b"x") + b"\x80\x00"))

    def test_adversarial_non_numeric_status(self):
        self.assertIsNone(pa.trailer_status(frame(0x80, b"grpc-status: 0; ok\r\n")))
        self.assertIsNone(pa.trailer_status(frame(0x80, b"grpc-status: -1\r\n")))


class EdgeReplyTable(unittest.TestCase):
    def test_positive_passed_from_trailer(self):
        r = pa.edge_reply(200, {}, frame(0x80, b"grpc-status: 0\r\n"))
        self.assertTrue(r.passed)
        self.assertFalse(r.refused)

    def test_positive_refused_by_jwt_authn(self):
        self.assertTrue(pa.edge_reply(401, {}, b"Jwt is missing").refused)

    def test_corner_trailers_only_header_wins(self):
        r = pa.edge_reply(200, {"Grpc-Status": "16"}, frame(0x80, b"grpc-status: 0\r\n"))
        self.assertEqual(r.grpc, 16)
        self.assertTrue(r.refused)

    def test_negative_other_error_is_neither(self):
        r = pa.edge_reply(200, {"grpc-status": "12"}, b"")
        self.assertFalse(r.passed or r.refused)

    def test_boundary_http_200_without_status_is_not_passed(self):
        self.assertFalse(pa.edge_reply(200, {}, b"").passed)

    def test_adversarial_garbage_status_header(self):
        r = pa.edge_reply(200, {"grpc-status": "0x0"}, b"")
        self.assertIsNone(r.grpc)
        self.assertFalse(r.passed)

    def test_positive_empty_request_frame(self):
        self.assertEqual(pa.grpc_web_frame(), b"\x00\x00\x00\x00\x00")


# ---------------------------------------------------------------- WebDriver


class WdValue(unittest.TestCase):
    def test_positive(self):
        self.assertEqual(pa.wd_value({"value": [1]}), [1])

    def test_corner_null_value(self):
        self.assertIsNone(pa.wd_value({"value": None}))

    def test_negative_error_answer(self):
        with self.assertRaisesRegex(ae.HarnessError, "no such window"):
            pa.wd_value({"value": {"error": "no such window", "message": "gone"}})

    def test_adversarial_not_an_object(self):
        for doc in ([], "x", None, {"status": 0}):
            with self.assertRaises(ae.HarnessError):
                pa.wd_value(doc)


class FakeDriver:
    """A WebDriver transport: scripted answers for execute/sync, recorded calls."""

    def __init__(self, scripts: list | None = None, session: object = None) -> None:
        self.calls: list[tuple[str, str, object]] = []
        self.scripts = list(scripts or [])
        self.session = {"sessionId": "s1"} if session is None else session

    def __call__(self, method: str, path: str, body: dict | None) -> object:
        self.calls.append((method, path, body))
        if path == "/session":
            return {"value": self.session}
        if path.endswith("/execute/sync"):
            return {"value": self.scripts.pop(0) if self.scripts else None}
        if path.endswith("/url") and method == "GET":
            return {"value": PORTAL}
        return {"value": None}


class BrowserTable(unittest.TestCase):
    def test_positive_starts_headless_with_the_given_binary(self):
        d = FakeDriver()
        pa.Browser(d, "/bin/chromium")
        caps = d.calls[0][2]["capabilities"]["alwaysMatch"]["goog:chromeOptions"]
        self.assertEqual(caps["binary"], "/bin/chromium")
        self.assertIn("--headless=new", caps["args"])

    def test_positive_texts_keeps_pairs_only(self):
        b = pa.Browser(FakeDriver([[["button", "Sign out"], ["", "x"], "junk", [1]]]), "c")
        self.assertEqual(b.texts(), ["Sign out", "x"])

    def test_negative_no_session_id(self):
        with self.assertRaises(ae.HarnessError):
            pa.Browser(FakeDriver(session={"capabilities": {}}), "c")

    def test_corner_click_retries_until_the_button_shows(self):
        # texts, click(False), texts, click(True)
        b = pa.Browser(FakeDriver([[], False, [], True]), "c", sleep=lambda _s: None)
        self.assertTrue(b.click("Sign out"))

    def test_boundary_wait_text_times_out(self):
        b = pa.Browser(FakeDriver([]), "c", sleep=lambda _s: None)
        self.assertFalse(b.wait_text("Sign out", timeout=0))

    def test_adversarial_storage_non_string_is_none(self):
        b = pa.Browser(FakeDriver([{"access_token": "x"}]), "c")
        self.assertIsNone(b.storage(pa.SESSION_KEY))

    def test_positive_click_label_passed_as_argument_not_code(self):
        d = FakeDriver([[], True])
        pa.Browser(d, "c").click("x'); alert(1); ('")
        body = d.calls[-1][2]
        self.assertEqual(body["args"], ["x'); alert(1); ('"])
        self.assertNotIn("alert", body["script"])


class StoredSession(unittest.TestCase):
    def test_positive(self):
        raw = json.dumps({"access_token": "t", "expires_at": 1, "refresh_handle": "h"})
        self.assertEqual(pa.stored_session(raw), {"access_token": "t", "refresh_handle": "h"})

    def test_negative_missing_handle(self):
        self.assertIsNone(pa.stored_session(json.dumps({"access_token": "t"})))

    def test_corner_absent_or_empty(self):
        self.assertIsNone(pa.stored_session(None))
        self.assertIsNone(pa.stored_session(json.dumps({"access_token": "", "refresh_handle": "h"})))

    def test_boundary_token_at_the_cap(self):
        ok = json.dumps({"access_token": "t" * pa.MAX_TOKEN, "refresh_handle": "h"})
        big = json.dumps({"access_token": "t" * (pa.MAX_TOKEN + 1), "refresh_handle": "h"})
        self.assertIsNotNone(pa.stored_session(ok))
        self.assertIsNone(pa.stored_session(big))

    def test_adversarial_not_json_or_not_object(self):
        for raw in ("{", "[]", '"t"', json.dumps({"access_token": 1, "refresh_handle": "h"}), "x" * (4 * pa.MAX_TOKEN)):
            self.assertIsNone(pa.stored_session(raw))


class CallbackParams(unittest.TestCase):
    def test_positive(self):
        self.assertTrue(pa.has_callback_params(PORTAL + "?code=c&state=s"))

    def test_negative_clean(self):
        self.assertFalse(pa.has_callback_params(PORTAL))

    def test_corner_error_alone(self):
        self.assertTrue(pa.has_callback_params(PORTAL + "?error=access_denied"))

    def test_boundary_fragment_is_not_query(self):
        self.assertFalse(pa.has_callback_params(PORTAL + "#code=c"))


# ---------------------------------------------------------------- config + wiring


def layout(work: str = "/w") -> pa.Layout:
    return pa.Layout(Path(work), "http://127.0.0.1:4000", 50500, 18090, 18092)


class RenderConfig(unittest.TestCase):
    def test_positive_browser_sign_in_for_the_portal(self):
        cfg = tomllib.loads(pa.render_config(layout(), Path("/w/secret")))
        self.assertEqual(cfg["auth"]["redirect_uris"], [PORTAL])
        issuer = cfg["auth"]["issuers"][0]
        self.assertEqual((issuer["name"], issuer["audience"]), (pa.ISSUER_NAME, pa.CLIENT_ID))
        self.assertEqual(cfg["grpc"]["tls"]["client_ca"], "/w/pki/ca/root.crt")

    def test_negative_secret_is_a_reference_never_a_value(self):
        cfg = tomllib.loads(pa.render_config(layout(), Path("/w/secret")))
        self.assertEqual(cfg["auth"]["issuers"][0]["client_secret"], "file:/w/secret")

    def test_corner_sessions_in_a_file_store_under_the_work_dir(self):
        cfg = tomllib.loads(pa.render_config(layout(), Path("/w/secret")))
        self.assertEqual(cfg["auth"]["token"]["session_path"], "/w/agent/auth-sessions")

    def test_boundary_redirect_uri_is_the_exact_origin_with_slash(self):
        lay = layout()
        self.assertEqual(lay.portal_url, lay.origin + "/")

    def test_adversarial_paths_cannot_break_out_of_strings(self):
        weird = '/w/"]\n[auth]\nmode = "none"'
        cfg = tomllib.loads(pa.render_config(layout(weird), Path(weird + "/s")))
        self.assertEqual(cfg["auth"]["mode"], "oidc")
        self.assertEqual(cfg["agent"]["working_dir"], weird + "/agent")


class EnvoyWiring(unittest.TestCase):
    def test_positive_spec_loads_in_the_renderer(self):
        spec = portal_envoy.Spec.load(pa.envoy_spec(layout(), 4317))
        self.assertEqual([(l.port, l.upstream_port) for l in spec.listeners], [(18090, 50500)])

    def test_positive_knobs_pass_the_renderers_validation(self):
        lay = layout()
        spec = portal_envoy.Spec.load(pa.envoy_spec(lay, 4317))
        k = portal_envoy.knobs_from_env(pa.envoy_env(lay, "podman", Path("/w/jwks.json")), spec)
        self.assertEqual((k.auth, k.host, k.origins), ("on", "127.0.0.1", (lay.origin,)))
        self.assertEqual(k.upstream_cert, "/w/pki/envoy/cert.pem")

    def test_negative_auth_is_never_off(self):
        self.assertEqual(pa.envoy_env(layout(), "docker", Path("/j"))["PORTAL_AUTH"], "on")

    def test_corner_web_build_points_at_the_edge(self):
        self.assertIn("--dart-define=PORTAL_GRPC_WEB_URL=http://127.0.0.1:18090", pa.dart_defines(layout()))
        self.assertIn("--dart-define=PORTAL_AUTH=on", pa.dart_defines(layout()))

    def test_adversarial_runtime_state_dir_untouched(self):
        # podman keeps crun's state under XDG_RUNTIME_DIR; overriding it for the
        # bring-up left a container nothing could stop once the work dir was gone.
        self.assertNotIn("XDG_RUNTIME_DIR", pa.envoy_env(layout(), "podman", Path("/j")))

    def test_corner_rendered_config_path_follows_portal_envoy(self):
        self.assertEqual(
            pa.rendered_config("n", {"XDG_RUNTIME_DIR": "/run/user/1"}), Path("/run/user/1/n-envoy.yaml")
        )
        self.assertEqual(pa.rendered_config("n", {}), Path("/tmp/n-envoy.yaml"))

    def test_boundary_every_path_absolute(self):
        env = pa.envoy_env(layout(), "docker", Path("/j"))
        for key in ("PORTAL_JWT_JWKS", "PORTAL_UPSTREAM_CA", "PORTAL_UPSTREAM_CERT", "PORTAL_UPSTREAM_KEY"):
            self.assertTrue(Path(env[key]).is_absolute(), key)


class MainSkips(unittest.TestCase):
    BASE = [
        "--portal-envoy", "p", "--proto-dir", "d", "--envoy-image", "i",
        "--envoy-name", "n", "--edge-port", "1", "--web-port", "2",
    ]

    def test_corner_no_runtime_is_a_skip(self):
        out = io.StringIO()
        with mock.patch.object(pa, "runtime_up", return_value=False), mock.patch("sys.stdout", out):
            self.assertEqual(pa.main([*self.BASE, "--runtime", "podman"]), ae.EXIT_OK)
        self.assertIn("SKIP", out.getvalue())

    def test_adversarial_unknown_runtime_is_harness(self):
        with mock.patch("sys.stderr", io.StringIO()):
            self.assertEqual(pa.main([*self.BASE, "--runtime", "sh -c id"]), ae.EXIT_HARNESS)


# ---------------------------------------------------------------- check-the-checks


class World:
    """A fake portal (the AuthState flow), agent and edge around a real CodeFlow.
    `breaks` names the one promise this world breaks."""

    def __init__(self, breaks: str = "") -> None:
        self.breaks = breaks
        self.flow = flow()
        self.sessions: dict[str, dict] = {}  # refresh handle → {token, alive}
        self.n = 0

    # -- agent
    def agent(self, method: str, data: dict | None = None, bearer: str | None = None) -> ae.Reply:
        if method.endswith("/WhoAmI"):
            live = any(s["token"] == bearer for s in self.sessions.values())
            if not live:
                return ae.Reply("Unauthenticated", {}, "")
            tenant = "tenant-b" if self.breaks == "wrong_tenant" else ae.TENANT_A
            sid = next(s["sid"] for s in self.sessions.values() if s["token"] == bearer)
            return ae.Reply("OK", {"tenant": tenant, "subject": f"user:fake/{pa.USER}", "sid": sid}, "")
        if method.endswith("/Refresh"):
            s = self.sessions.get((data or {}).get("refreshHandle", ""))
            ok = s is not None and (s["alive"] or self.breaks == "signout_keeps_session")
            return ae.Reply("OK" if ok else "Unauthenticated", {}, "")
        raise AssertionError(method)

    # -- edge (and the agent behind it: a scoped service needs the session header)
    def edge(self, path: str, bearer: str | None, session: str | None = None) -> pa.EdgeReply:
        if path == pa.HEALTH_RPC:
            return pa.EdgeReply(200, 0)
        valid = any(
            s["token"] == bearer and s["alive"] and s["sid"] == session for s in self.sessions.values()
        )
        if self.breaks == "edge_admits_forged" and bearer is not None:
            valid = True
        if self.breaks == "edge_refuses_token":
            valid = False
        return pa.EdgeReply(200, 0) if valid else pa.EdgeReply(401, None)

    def exchange(self, code: str, verifier: str) -> dict | None:
        answer = self.flow.redeem(
            urllib.parse.urlencode(
                {
                    "grant_type": "authorization_code",
                    "client_id": pa.CLIENT_ID,
                    "client_secret": SECRET,
                    "code": code,
                    "redirect_uri": PORTAL,
                    "code_verifier": verifier,
                }
            )
        )
        if answer.status != 200:
            return None
        self.n += 1
        s = {"access_token": f"tok-{self.n}", "refresh_handle": f"h-{self.n}"}
        self.sessions[s["refresh_handle"]] = {"token": s["access_token"], "alive": True, "sid": f"sid-{self.n}"}
        return s


class FakeBrowser:
    """The portal as the browser shows it, modelled on AuthState."""

    def __init__(self, world: World) -> None:
        self.w = world
        self.url = ""
        self.store: dict[str, str] = {}
        self.page = "blank"
        self.error = ""

    def open(self, url: str) -> None:
        self.url = url
        self._boot()

    def reload(self) -> None:
        if self.w.breaks == "reload_goes_to_idp" and pa.SESSION_KEY in self.store:
            self.store.pop(pa.SESSION_KEY)
            self.page = "login"
            self.click(pa.SIGN_IN_BUTTON)
            return
        self._boot()

    def _boot(self) -> None:
        q = {k: v[0] for k, v in urllib.parse.parse_qs(urllib.parse.urlsplit(self.url).query).items()}
        self.error = ""
        if {"code", "state", "error"} & q.keys():
            pending = json.loads(self.store.pop(pa.PENDING_KEY, "null"))
            if self.w.breaks == "keep_pending" and pending is not None:
                self.store[pa.PENDING_KEY] = json.dumps(pending)
            if self.w.breaks != "callback_left_in_url":
                self.url = PORTAL
            if "error" in q:
                self.page = "login"
                self.error = (
                    "Sign-in failed." if self.w.breaks == "idp_refusal_hidden"
                    else f"The identity provider did not sign you in ({q['error']})."
                )
                return
            matches = pending is not None and pending["state"] == q.get("state")
            if not matches and self.w.breaks != "replay_redeemed":
                self.page = "login"
                self.error = "That sign-in did not start in this tab, or was already used. Sign in again."
                return
            verifier = pending["verifier"] if pending else "x" * 43
            s = self.w.exchange(q.get("code", ""), verifier)
            if s is None:
                self.page = "login"
                self.error = "That sign-in did not start in this tab, or was already used. Sign in again."
                return
            self.store[pa.SESSION_KEY] = json.dumps(s)
            self.page = "app"
            return
        self.page = "app" if pa.SESSION_KEY in self.store else "login"

    def current_url(self) -> str:
        return self.url

    def _prompts(self) -> str:
        """The Prompts page lists through the edge with the portal's headers."""
        s = json.loads(self.store[pa.SESSION_KEY])
        me = self.w.agent("agent.v1.AuthService/WhoAmI", {}, bearer=s["access_token"])
        sid = None if self.w.breaks == "no_session_header" else me.body.get("sid")
        got = self.w.edge(pa.GATED_RPC, s["access_token"], sid)
        return pa.PROMPTS_LOADED if got.passed else f"{pa.PROMPTS_FAILED}.\ngRPC Error (16)"

    def texts(self) -> list[str]:
        if self.page == "app":
            return [pa.PROMPTS_NAV, pa.SIGN_OUT_BUTTON]
        if self.page == "prompts":
            return [pa.PROMPTS_NAV, self._prompts(), pa.SIGN_OUT_BUTTON]
        out = ["Sign in to Agent Seddon"]
        if self.error:
            out.append(self.error)
        if self.w.breaks != "no_issuer":
            out.append(pa.SIGN_IN_BUTTON)
        if self.w.breaks == "signed_in_early":
            out.append(pa.SIGN_OUT_BUTTON)
        return out

    def wait_text(self, want: str, timeout: float = 0) -> bool:
        return any(want in t for t in self.texts())

    def click(self, label: str, timeout: float = 0) -> bool:
        if label not in self.texts():
            return False
        if label == pa.PROMPTS_NAV:
            self.page = "prompts"
            return True
        if label == pa.SIGN_OUT_BUTTON:
            s = json.loads(self.store.pop(pa.SESSION_KEY))
            self.w.sessions[s["refresh_handle"]]["alive"] = False
            if self.w.breaks == "signout_keeps_storage":
                self.store[pa.SESSION_KEY] = json.dumps(s)
            self.page = "login"
            return True
        verifier = secrets.token_urlsafe(32)
        state = secrets.token_urlsafe(8)
        self.store[pa.PENDING_KEY] = json.dumps({"state": state, "verifier": verifier})
        redirect = "http://127.0.0.1:1/" if self.w.breaks == "wrong_redirect" else PORTAL
        answer = self.w.flow.authorize(
            urllib.parse.urlencode(
                {
                    "response_type": "code",
                    "client_id": pa.CLIENT_ID,
                    "redirect_uri": redirect,
                    "code_challenge": pa.s256(verifier),
                    "code_challenge_method": "S256",
                    "state": state,
                    "nonce": "" if self.w.breaks == "no_nonce" else "n",
                }
            )
        )
        if answer.status == 302:
            self.open(answer.location)
        else:
            self.page, self.error = "login", "The identity provider refused."
        return True

    def storage(self, key: str) -> str | None:
        return self.store.get(key)


def run_world(breaks: str = "") -> tuple[int, str]:
    w = World(breaks)
    ctx = pa.Ctx(browser=FakeBrowser(w), flow=w.flow, edge=w.edge, agent=w.agent, portal_url=PORTAL)
    out = io.StringIO()
    return ae.run_steps(ctx, pa.STEPS, out=out), out.getvalue()


class CheckTheChecks(unittest.TestCase):
    def test_positive_a_behaving_world_passes_every_step(self):
        code, out = run_world()
        self.assertEqual(code, ae.EXIT_OK, out)
        self.assertEqual(out.count("ok   "), len(pa.STEPS))

    # (broken promise, the step that must go red)
    BROKEN = [
        ("edge_admits_forged", "the edge refuses anonymous calls"),
        ("no_issuer", "the sign-in page offers the issuer"),
        ("signed_in_early", "the sign-in page offers the issuer"),
        ("wrong_redirect", "browser sign-in through the IdP"),
        ("no_nonce", "browser sign-in through the IdP"),
        ("callback_left_in_url", "browser sign-in through the IdP"),
        ("keep_pending", "browser sign-in through the IdP"),
        ("wrong_tenant", "browser sign-in through the IdP"),
        ("edge_refuses_token", "the portal's token passes the edge"),
        ("no_session_header", "signed-in pages load"),
        ("reload_goes_to_idp", "a reload resumes the session"),
        ("replay_redeemed", "replayed and forged callbacks are refused"),
        ("idp_refusal_hidden", "an IdP refusal is shown"),
        ("signout_keeps_storage", "sign-out revokes the session"),
        ("signout_keeps_session", "sign-out revokes the session"),
    ]

    def test_adversarial_every_broken_promise_fails_its_step(self):
        for breaks, step in self.BROKEN:
            with self.subTest(breaks=breaks):
                code, out = run_world(breaks)
                self.assertEqual(code, ae.EXIT_CONTRACT, out)
                self.assertIn(f"FAIL {step}:", out)

    def test_boundary_every_step_is_covered_by_a_broken_world(self):
        covered = {step for _, step in self.BROKEN}
        self.assertEqual(covered, {name for name, _ in pa.STEPS})


if __name__ == "__main__":
    unittest.main()
