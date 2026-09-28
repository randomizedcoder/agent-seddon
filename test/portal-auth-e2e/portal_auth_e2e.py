"""portal-auth-e2e: browser sign-in through the hardened edge (security-hardening S15c).

S15a (`auth-e2e`) and S15b (`auth-integration`) drive the sign-in chain with
grpcurl. This harness drives it the way a person does: a real headless Chromium,
talking W3C WebDriver to chromedriver, loads the real portal web build and signs in.
On loopback only, it stands up:

- a fake OIDC identity provider with the authorization-code flow: discovery, JWKS,
  `/authorize` (consents at once, `302` back to the portal) and `/token` (checks
  the client secret, the redirect URI and the PKCE verifier, each code once);
- an offline dev PKI (`pki-dev`): CA, the agent's and Envoy's leaves, the token
  signer;
- `agent --serve-all` over mTLS, `mode = "oidc"`, browser sign-in on
  (`[auth] redirect_uris` = the portal's origin), sessions in a file store;
- the hardened Envoy bridge (`portal_envoy.py up`, S14): `jwt_authn` against the
  agent's JWKS, exact-origin CORS, loopback bind, upstream mTLS to the agent;
- the portal web bundle (`flutter build web`, `PORTAL_AUTH=on`) behind
  `static-web-server`,

then checks the contract of docs/design/security-hardening/06-portal-and-edge.md:

- the edge refuses a gated RPC with no bearer or a forged one;
- the sign-in page lists the issuer the agent offers;
- one click goes to the IdP and back: a PKCE `S256` authorization request for the
  portal's own redirect URI, the code redeemed by the agent with the client secret,
  the portal signed in as the tenant the IdP asserted, `?code&state` gone from the
  address bar;
- that token passes the edge;
- a reload resumes the session without going back to the IdP;
- a replayed or forged callback is refused without asking the IdP;
- an IdP refusal is shown and nothing is redeemed;
- sign-out revokes the session: its refresh handle is refused.

Exit codes follow nix/lib/contract.sh: 0 ok (or skipped with no container
runtime), 1 harness (the setup broke), 2 contract (the portal or agent answered
wrongly).
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import secrets
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Callable, Mapping, Sequence

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "auth-e2e"))

import auth_e2e as e2e  # noqa: E402

from auth_e2e import (  # noqa: E402
    EXIT_CONTRACT,
    EXIT_HARNESS,
    EXIT_OK,
    TENANT_A,
    TOKEN_AUDIENCE,
    TOKEN_ISSUER,
    Client,
    ContractError,
    HarnessError,
    Reply,
    expect,
    toml_str,
)

ISSUER_NAME = "fake"
CLIENT_ID = "portal-e2e"
USER = "alice"
# The portal's tab storage keys (portal/lib/src/auth/auth_state.dart).
SESSION_KEY = "agent-seddon.session"
PENDING_KEY = "agent-seddon.signin"
# What the portal shows (portal/lib/src/pages/login_page.dart, auth_gate.dart).
SIGN_IN_BUTTON = f"Continue with {ISSUER_NAME}"
SIGN_OUT_BUTTON = "Sign out"
NOT_THIS_TAB = "did not start in this tab"
# The Prompts page (portal/lib/src/pages/prompts_page.dart): the system prompt's row
# is drawn only from a `PromptService/List` answer; the error panel otherwise.
PROMPTS_NAV = "Prompts"
PROMPTS_LOADED = "(system prompt)"
PROMPTS_FAILED = "Not connected to the gateway"
IDP_REFUSED = "did not sign you in (access_denied)"
# A read `--serve-all` always serves and an org admin may make (`read:prompt`);
# the edge's jwt_authn guards it.
GATED_RPC = "/agent.v1.PromptService/List"
HEALTH_RPC = "/grpc.health.v1.Health/Check"
MAX_BODY = 1 << 20
MAX_FORM = 64 * 1024
MAX_TOKEN = 16 * 1024
UI_WAIT = 30.0
EDGE_WAIT = 180.0  # the first run pulls the Envoy image


# ---------------------------------------------------------------- the fake IdP


def s256(verifier: str) -> str:
    """The PKCE `S256` challenge of a verifier (RFC 7636 §4.2)."""
    digest = hashlib.sha256(verifier.encode()).digest()
    return base64.urlsafe_b64encode(digest).rstrip(b"=").decode()


def with_query(url: str, params: Mapping[str, str]) -> str:
    """`url` with `params` appended to its query, anything already there kept."""
    parts = urllib.parse.urlsplit(url)
    query = urllib.parse.parse_qsl(parts.query, keep_blank_values=True) + list(params.items())
    return urllib.parse.urlunsplit(parts._replace(query=urllib.parse.urlencode(query)))


def single_values(raw: str) -> dict[str, str] | None:
    """A query string or form body as a map, or None when any key repeats: an
    OAuth parameter sent twice is refused (RFC 6749 §3.1), never guessed."""
    pairs = urllib.parse.parse_qsl(raw, keep_blank_values=True)
    out: dict[str, str] = {}
    for k, v in pairs:
        if k in out:
            return None
        out[k] = v
    return out


@dataclass(frozen=True)
class Answer:
    """One IdP HTTP answer: status, the `Location` of a redirect, a JSON body."""

    status: int
    location: str = ""
    body: dict = field(default_factory=dict)


@dataclass
class Issued:
    challenge: str
    redirect_uri: str
    nonce: str


class CodeFlow:
    """The authorization-code state of the fake IdP, free of HTTP so the tables can
    drive it. `/authorize` consents at once (or refuses once, after `deny_next`);
    `/token` redeems each code once, for the client, secret, redirect URI and PKCE
    verifier it was issued against. Every request and its outcome is recorded."""

    def __init__(
        self,
        client_id: str,
        client_secret: str,
        redirect_uris: Sequence[str],
        mint: Callable[[dict], str],
        claims: Callable[[str], dict],
    ) -> None:
        self.client_id = client_id
        self.client_secret = client_secret
        self.redirect_uris = tuple(redirect_uris)
        self._mint = mint
        self._claims = claims
        self._issued: dict[str, Issued] = {}
        self._lock = threading.Lock()
        self.deny_next: str | None = None
        # (query, Location answered or "", outcome)
        self.authorizations: list[tuple[dict, str, str]] = []
        # (form, outcome)
        self.redemptions: list[tuple[dict, str]] = []

    def authorize(self, raw_query: str) -> Answer:
        with self._lock:
            return self._authorize(raw_query)

    def _authorize(self, raw_query: str) -> Answer:
        q = single_values(raw_query)

        def refuse(reason: str) -> Answer:
            self.authorizations.append((q or {}, "", reason))
            return Answer(400, body={"error": reason})

        if q is None:
            return refuse("invalid_request")
        redirect_uri = q.get("redirect_uri", "")
        # A redirect URI that is not registered is never followed (RFC 6749 §4.1.2.1).
        if redirect_uri not in self.redirect_uris:
            return refuse("invalid_redirect_uri")
        if q.get("client_id") != self.client_id:
            return refuse("invalid_client")
        if q.get("response_type") != "code":
            return refuse("unsupported_response_type")
        if q.get("code_challenge_method") != "S256" or not q.get("code_challenge"):
            return refuse("invalid_request")
        state = q.get("state", "")
        if self.deny_next is not None:
            error, self.deny_next = self.deny_next, None
            location = with_query(redirect_uri, {"error": error, "state": state})
            self.authorizations.append((q, location, "denied"))
            return Answer(302, location)
        code = secrets.token_urlsafe(24)
        self._issued[code] = Issued(q["code_challenge"], redirect_uri, q.get("nonce", ""))
        location = with_query(redirect_uri, {"code": code, "state": state})
        self.authorizations.append((q, location, "consented"))
        return Answer(302, location)

    def redeem(self, raw_form: str) -> Answer:
        with self._lock:
            return self._redeem(raw_form)

    def _redeem(self, raw_form: str) -> Answer:
        f = single_values(raw_form)

        def refuse(reason: str) -> Answer:
            self.redemptions.append((f or {}, reason))
            return Answer(400, body={"error": reason})

        if f is None:
            return refuse("invalid_request")
        if f.get("grant_type") != "authorization_code":
            return refuse("unsupported_grant_type")
        if f.get("client_id") != self.client_id or f.get("client_secret") != self.client_secret:
            return refuse("invalid_client")
        issued = self._issued.pop(f.get("code", ""), None)
        if issued is None:
            return refuse("invalid_grant")
        if issued.redirect_uri != f.get("redirect_uri") or s256(f.get("code_verifier", "")) != issued.challenge:
            return refuse("invalid_grant")
        self.redemptions.append((f, "ok"))
        claims = self._claims(issued.nonce)
        return Answer(200, body={"access_token": "idp-access", "token_type": "Bearer", "id_token": self._mint(claims)})


class CodeIssuer(e2e.Issuer):
    """The S15a fake issuer plus `/authorize` and `/token`, backed by a [CodeFlow]."""

    def __init__(self, client_secret: str, redirect_uris: Sequence[str]) -> None:
        super().__init__()
        self.flow = CodeFlow(
            CLIENT_ID,
            client_secret,
            redirect_uris,
            self.mint,
            lambda nonce: {
                **e2e.login_claims(self.url, USER, TENANT_A, audience=CLIENT_ID),
                "nonce": nonce,
            },
        )

    def discovery(self) -> dict:
        return {
            **super().discovery(),
            "authorization_endpoint": f"{self.url}/authorize",
            "token_endpoint": f"{self.url}/token",
            "response_types_supported": ["code"],
            "code_challenge_methods_supported": ["S256"],
        }

    def _handler(self) -> type[BaseHTTPRequestHandler]:
        issuer = self

        class Handler(BaseHTTPRequestHandler):
            def _send(self, answer: Answer) -> None:
                body = json.dumps(answer.body).encode() if answer.body else b""
                self.send_response(answer.status)
                if answer.location:
                    self.send_header("location", answer.location)
                if body:
                    self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(body)))
                self.send_header("cache-control", "no-store")
                self.end_headers()
                self.wfile.write(body)

            def do_GET(self) -> None:  # noqa: N802 (http.server's name)
                path, _, query = self.path.partition("?")
                if path == "/authorize":
                    self._send(issuer.flow.authorize(query))
                elif path == "/.well-known/openid-configuration":
                    self._send(Answer(200, body=issuer.discovery()))
                elif path == "/jwks":
                    self._send(Answer(200, body=issuer.jwks()))
                else:
                    self.send_error(404)

            def do_POST(self) -> None:  # noqa: N802
                if self.path != "/token":
                    self.send_error(404)
                    return
                try:
                    n = int(self.headers.get("content-length", "0"))
                except ValueError:
                    n = -1
                if not 0 <= n <= MAX_FORM:
                    self._send(Answer(413, body={"error": "invalid_request"}))
                    return
                self._send(issuer.flow.redeem(self.rfile.read(n).decode("utf-8", "replace")))

            def log_message(self, *_args) -> None:
                pass

        return Handler


# ---------------------------------------------------------------- the edge (grpc-web)


def grpc_web_frame(message: bytes = b"") -> bytes:
    """One grpc-web data frame: flag 0, big-endian length, the message."""
    return b"\x00" + len(message).to_bytes(4, "big") + message


def parse_status(value: str) -> int | None:
    """A `grpc-status` value (0..16), or None for anything else."""
    value = value.strip()
    if not value.isdigit() or len(value) > 2:
        return None
    n = int(value)
    return n if n <= 16 else None


def trailer_status(body: bytes) -> int | None:
    """The `grpc-status` in a grpc-web body's trailer frame (flag bit 0x80). The
    body is the edge's answer, so it is untrusted: a frame whose length runs past
    the body, or a body that ends mid-header, reads as no status at all."""
    i = 0
    while i < len(body):
        if len(body) - i < 5:
            return None
        flag = body[i]
        n = int.from_bytes(body[i + 1 : i + 5], "big")
        start, end = i + 5, i + 5 + n
        if end > len(body):
            return None
        if flag & 0x80:
            for line in body[start:end].decode("latin-1").split("\r\n"):
                name, sep, value = line.partition(":")
                if sep and name.strip().lower() == "grpc-status":
                    return parse_status(value)
            return None
        i = end
    return None


@dataclass(frozen=True)
class EdgeReply:
    http: int
    grpc: int | None

    @property
    def refused(self) -> bool:
        """The edge (or the agent behind it) said: no valid credential."""
        return self.http in (401, 403) or self.grpc in (7, 16)

    @property
    def passed(self) -> bool:
        return self.http == 200 and self.grpc == 0


def edge_reply(http: int, headers: Mapping[str, str], body: bytes) -> EdgeReply:
    """Classify one grpc-web answer: a trailers-only answer carries `grpc-status`
    as a header, a full one in the body's trailer frame."""
    lowered = {k.lower(): v for k, v in headers.items()}
    if "grpc-status" in lowered:
        return EdgeReply(http, parse_status(lowered["grpc-status"]))
    return EdgeReply(http, trailer_status(body))


def call_edge(base: str, origin: str, path: str, bearer: str | None, session: str | None = None) -> EdgeReply:
    """POST one empty-request grpc-web call through the edge, as the portal does:
    the bearer, and the auth session id as `x-agent-session-id`."""
    headers = {
        "content-type": "application/grpc-web+proto",
        "accept": "application/grpc-web+proto",
        "x-grpc-web": "1",
        "origin": origin,
    }
    if bearer is not None:
        headers["authorization"] = f"Bearer {bearer}"
    if session is not None:
        headers["x-agent-session-id"] = session
    req = urllib.request.Request(base + path, data=grpc_web_frame(), headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            return edge_reply(resp.status, dict(resp.headers), resp.read(MAX_BODY))
    except urllib.error.HTTPError as e:
        return edge_reply(e.code, dict(e.headers or {}), e.read(MAX_BODY))
    except (urllib.error.URLError, OSError) as e:
        raise HarnessError(f"the edge at {base} did not answer: {e}") from e


# ---------------------------------------------------------------- the browser (WebDriver)

# Flutter web keeps its widgets in a canvas; the accessibility tree mirrors them
# as `flt-semantics` nodes once semantics is on. The placeholder turns it on.
# Every node is read, containers too: a text node can have element children.
JS_TEXTS = """
const p = document.querySelector('flt-semantics-placeholder');
if (p) { p.click(); }
const out = [];
for (const n of document.querySelectorAll('flt-semantics')) {
  const t = ((n.getAttribute('aria-label') || '') + ' ' + (n.textContent || '')).trim();
  if (t) out.push([n.getAttribute('role') || '', t]);
}
return out;
"""
JS_CLICK = """
const want = arguments[0];
const nodes = Array.from(document.querySelectorAll('flt-semantics[role]'))
  .filter(n => ['button', 'tab', 'link', 'menuitem'].includes(n.getAttribute('role')));
const label = n => ((n.getAttribute('aria-label') || '') + ' ' + (n.textContent || '')).trim();
const hit = nodes.find(n => label(n) === want) || nodes.find(n => label(n).includes(want));
if (hit) { hit.click(); return true; }
return false;
"""
JS_STORAGE = "return window.sessionStorage.getItem(arguments[0]);"


def capabilities(chromium: str) -> dict:
    return {
        "capabilities": {
            "alwaysMatch": {
                "browserName": "chrome",
                "goog:chromeOptions": {
                    "binary": chromium,
                    "args": [
                        "--headless=new",
                        "--no-sandbox",
                        "--disable-gpu",
                        "--disable-dev-shm-usage",
                        "--window-size=1400,1000",
                    ],
                },
            }
        }
    }


def wd_value(doc: object) -> object:
    """The `value` of a WebDriver answer; an error answer is a harness error."""
    if not isinstance(doc, dict) or "value" not in doc:
        raise HarnessError(f"chromedriver answered without a value: {str(doc)[:200]}")
    value = doc["value"]
    if isinstance(value, dict) and "error" in value:
        raise HarnessError(f"webdriver {value.get('error')}: {str(value.get('message', ''))[:300]}")
    return value


Transport = Callable[[str, str, "dict | None"], object]


def http_transport(base: str) -> Transport:
    def send(method: str, path: str, body: dict | None) -> object:
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(base + path, data=data, method=method, headers={"content-type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                return json.loads(resp.read(MAX_BODY))
        except urllib.error.HTTPError as e:
            try:
                return json.loads(e.read(MAX_BODY))
            except ValueError:
                raise HarnessError(f"chromedriver {method} {path}: HTTP {e.code}") from e
        except (urllib.error.URLError, OSError, ValueError) as e:
            raise HarnessError(f"chromedriver {method} {path}: {e}") from e

    return send


class Browser:
    """One headless Chromium tab over W3C WebDriver."""

    def __init__(self, send: Transport, chromium: str, sleep: Callable[[float], None] = time.sleep) -> None:
        self._send = send
        self._sleep = sleep
        started = wd_value(send("POST", "/session", capabilities(chromium)))
        if not isinstance(started, dict) or not isinstance(started.get("sessionId"), str):
            raise HarnessError(f"chromedriver started no session: {str(started)[:200]}")
        self._sid = started["sessionId"]

    def _s(self, method: str, path: str, body: dict | None = None) -> object:
        return wd_value(self._send(method, f"/session/{self._sid}{path}", body))

    def open(self, url: str) -> None:
        self._s("POST", "/url", {"url": url})

    def reload(self) -> None:
        self._s("POST", "/refresh", {})

    def current_url(self) -> str:
        return str(self._s("GET", "/url"))

    def script(self, js: str, *args: object) -> object:
        return self._s("POST", "/execute/sync", {"script": js, "args": list(args)})

    def texts(self) -> list[str]:
        got = self.script(JS_TEXTS)
        return [str(t[1]) for t in got if isinstance(t, list) and len(t) == 2] if isinstance(got, list) else []

    def wait_text(self, want: str, timeout: float = UI_WAIT) -> bool:
        deadline = time.monotonic() + timeout
        while True:
            if any(want in t for t in self.texts()):
                return True
            if time.monotonic() >= deadline:
                return False
            self._sleep(0.5)

    def click(self, label: str, timeout: float = UI_WAIT) -> bool:
        deadline = time.monotonic() + timeout
        while True:
            self.texts()  # semantics on
            if self.script(JS_CLICK, label) is True:
                return True
            if time.monotonic() >= deadline:
                return False
            self._sleep(0.5)

    def storage(self, key: str) -> str | None:
        got = self.script(JS_STORAGE, key)
        return got if isinstance(got, str) else None

    def close(self) -> None:
        try:
            self._send("DELETE", f"/session/{self._sid}", None)
        except HarnessError:
            pass


def stored_session(raw: str | None) -> dict | None:
    """The portal's stored session (tab storage, so untrusted): a JSON object with
    a string `access_token` and `refresh_handle` of sane size, else None."""
    if raw is None or len(raw) > 2 * MAX_TOKEN:
        return None
    try:
        doc = json.loads(raw)
    except ValueError:
        return None
    if not isinstance(doc, dict):
        return None
    token, handle = doc.get("access_token"), doc.get("refresh_handle")
    if not isinstance(token, str) or not isinstance(handle, str) or not token or not handle:
        return None
    if len(token) > MAX_TOKEN or len(handle) > MAX_TOKEN:
        return None
    return {"access_token": token, "refresh_handle": handle}


def has_callback_params(url: str) -> bool:
    q = urllib.parse.parse_qs(urllib.parse.urlsplit(url).query)
    return any(k in q for k in ("code", "state", "error"))


# ---------------------------------------------------------------- the contract


@dataclass
class Ctx:
    browser: Browser
    flow: CodeFlow
    edge: Callable[..., EdgeReply]  # (path, bearer, session=None)
    agent: Callable[..., Reply]  # (method, data, bearer=None) → grpcurl to the agent
    portal_url: str
    state: dict = field(default_factory=dict)


def _page(ctx: Ctx) -> str:
    seen: list[str] = []
    for t in ctx.browser.texts():
        if t not in seen:
            seen.append(t)
    return " | ".join(seen)[:400]


def _wait(ctx: Ctx, text: str, what: str) -> None:
    if not ctx.browser.wait_text(text):
        raise ContractError(f"{what}: {text!r} never showed; the page says: {_page(ctx)}")


def _click(ctx: Ctx, label: str) -> None:
    if not ctx.browser.click(label):
        raise ContractError(f"no {label!r} button to press; the page says: {_page(ctx)}")


def step_edge_refuses_anonymous(ctx: Ctx) -> None:
    health = ctx.edge(HEALTH_RPC, None)
    if not health.passed:
        raise ContractError(f"health through the edge should pass without a bearer: {health}")
    for bearer, what in ((None, "no bearer"), ("e30.e30.c2ln", "a forged bearer")):
        got = ctx.edge(GATED_RPC, bearer)
        if not got.refused:
            raise ContractError(f"{GATED_RPC} with {what} went through the edge: {got}")


def step_signin_page_offers_issuer(ctx: Ctx) -> None:
    ctx.browser.open(ctx.portal_url)
    _wait(ctx, SIGN_IN_BUTTON, "the sign-in page")
    if any(SIGN_OUT_BUTTON in t for t in ctx.browser.texts()):
        raise ContractError("the portal shows a signed-in account before anyone signed in")


def _sign_in(ctx: Ctx, what: str) -> dict:
    """Press the issuer's button and land signed in; returns the stored session."""
    _click(ctx, SIGN_IN_BUTTON)
    _wait(ctx, SIGN_OUT_BUTTON, what)
    session = stored_session(ctx.browser.storage(SESSION_KEY))
    if session is None:
        raise ContractError(f"{what}: the portal is signed in but stored no session")
    return session


def step_browser_signin(ctx: Ctx) -> None:
    n_auth, n_redeem = len(ctx.flow.authorizations), len(ctx.flow.redemptions)
    session = _sign_in(ctx, "sign-in through the IdP")
    new_auth = ctx.flow.authorizations[n_auth:]
    if len(new_auth) != 1 or new_auth[0][2] != "consented":
        raise ContractError(f"want one consented authorization request, got {[a[2] for a in new_auth]}")
    query, location, _ = new_auth[0]
    if query.get("redirect_uri") != ctx.portal_url:
        raise ContractError(f"the IdP was asked to return to {query.get('redirect_uri')!r}, not the portal")
    if not query.get("state") or not query.get("nonce"):
        raise ContractError("the authorization request carried no state or no nonce")
    redeemed = [r for r in ctx.flow.redemptions[n_redeem:] if r[1] == "ok"]
    if len(redeemed) != 1:
        raise ContractError(f"want the code redeemed once, got {[r[1] for r in ctx.flow.redemptions[n_redeem:]]}")
    if has_callback_params(ctx.browser.current_url()):
        raise ContractError(f"?code&state left in the address bar: {ctx.browser.current_url()}")
    if ctx.browser.storage(PENDING_KEY) is not None:
        raise ContractError("the pending sign-in (PKCE verifier) was left in tab storage")
    me = expect(ctx.agent("agent.v1.AuthService/WhoAmI", {}, bearer=session["access_token"]), "OK", "WhoAmI")
    if me.body.get("tenant") != TENANT_A or USER not in str(me.body.get("subject", "")):
        raise ContractError(f"signed in as the wrong principal: {me.body}")
    if not me.body.get("sid"):
        raise ContractError(f"WhoAmI named no auth session: {me.body}")
    ctx.state.update(session=session, callback=location, sid=me.body["sid"])


def step_token_passes_edge(ctx: Ctx) -> None:
    got = ctx.edge(GATED_RPC, ctx.state["session"]["access_token"], ctx.state["sid"])
    if not got.passed:
        raise ContractError(f"{GATED_RPC} with the portal's token did not pass the edge: {got}")


def step_signed_in_pages_load(ctx: Ctx) -> None:
    """The signed-in portal's own calls get through: its Prompts page (a scoped
    service, so the agent needs the verified tenant and a session) lists."""
    _click(ctx, PROMPTS_NAV)
    deadline = time.monotonic() + UI_WAIT
    while True:
        texts = ctx.browser.texts()
        failed = [t for t in texts if PROMPTS_FAILED in t]
        if failed:
            # The innermost node: containers repeat the whole page's text.
            raise ContractError(f"the signed-in Prompts page could not list: {min(failed, key=len)[:300]}")
        if any(PROMPTS_LOADED in t for t in texts):
            return
        if time.monotonic() >= deadline:
            raise ContractError(f"the Prompts page never loaded; the page says: {_page(ctx)}")
        time.sleep(0.5)


def step_reload_resumes(ctx: Ctx) -> None:
    n_auth = len(ctx.flow.authorizations)
    ctx.browser.reload()
    _wait(ctx, SIGN_OUT_BUTTON, "the reloaded portal")
    if len(ctx.flow.authorizations) != n_auth:
        raise ContractError("a reload went back to the IdP instead of resuming the session")


def step_callback_replay_refused(ctx: Ctx) -> None:
    n_redeem = len(ctx.flow.redemptions)
    forged = with_query(ctx.portal_url, {"code": "forged-code", "state": secrets.token_urlsafe(16)})
    for url, what in ((ctx.state["callback"], "a replayed callback"), (forged, "a forged callback")):
        ctx.browser.open(url)
        _wait(ctx, NOT_THIS_TAB, what)
        if any(SIGN_OUT_BUTTON in t for t in ctx.browser.texts()):
            raise ContractError(f"{what} left the portal signed in")
    if len(ctx.flow.redemptions) != n_redeem:
        raise ContractError("a replayed or forged callback reached the IdP's token endpoint")


def step_idp_refusal_shown(ctx: Ctx) -> None:
    n_redeem = len(ctx.flow.redemptions)
    ctx.flow.deny_next = "access_denied"
    _click(ctx, SIGN_IN_BUTTON)
    _wait(ctx, IDP_REFUSED, "an IdP refusal")
    if len(ctx.flow.redemptions) != n_redeem:
        raise ContractError("the portal redeemed something after the IdP refused")
    if ctx.browser.storage(PENDING_KEY) is not None:
        raise ContractError("the refused sign-in was left pending in tab storage")


def step_signout_revokes(ctx: Ctx) -> None:
    session = _sign_in(ctx, "signing in again")
    _click(ctx, SIGN_OUT_BUTTON)
    _wait(ctx, SIGN_IN_BUTTON, "sign-out")
    if ctx.browser.storage(SESSION_KEY) is not None:
        raise ContractError("sign-out left the session in tab storage")
    expect(
        ctx.agent("agent.v1.AuthService/Refresh", {"refreshHandle": session["refresh_handle"]}),
        "Unauthenticated",
        "Refresh with the handle of a signed-out session",
    )


STEPS: tuple[tuple[str, Callable[[Ctx], None]], ...] = (
    ("the edge refuses anonymous calls", step_edge_refuses_anonymous),
    ("the sign-in page offers the issuer", step_signin_page_offers_issuer),
    ("browser sign-in through the IdP", step_browser_signin),
    ("the portal's token passes the edge", step_token_passes_edge),
    ("signed-in pages load", step_signed_in_pages_load),
    ("a reload resumes the session", step_reload_resumes),
    ("replayed and forged callbacks are refused", step_callback_replay_refused),
    ("an IdP refusal is shown", step_idp_refusal_shown),
    ("sign-out revokes the session", step_signout_revokes),
)


# ---------------------------------------------------------------- set-up


@dataclass(frozen=True)
class Layout:
    work: Path
    issuer_url: str
    agent_port: int
    edge_port: int
    web_port: int

    @property
    def pki(self) -> Path:
        return self.work / "pki"

    def leaf(self, service: str) -> tuple[Path, Path]:
        return self.pki / service / "cert.pem", self.pki / service / "key.pem"

    @property
    def ca(self) -> Path:
        return self.pki / "ca" / "root.crt"

    @property
    def agent_addr(self) -> str:
        return f"127.0.0.1:{self.agent_port}"

    @property
    def edge_url(self) -> str:
        return f"http://127.0.0.1:{self.edge_port}"

    @property
    def portal_url(self) -> str:
        """The portal's own address: its origin, its CORS origin and the redirect URI."""
        return f"http://127.0.0.1:{self.web_port}/"

    @property
    def origin(self) -> str:
        return self.portal_url.rstrip("/")


def render_config(lay: Layout, secret_file: Path) -> str:
    """`agent.toml` for the one agent: `--serve-all` over mTLS, OIDC with browser
    sign-in for the portal's address, agent tokens, file sessions."""
    state = lay.work / "agent"
    cert, key = lay.leaf("agent")
    return "\n".join(
        [
            "[agent]",
            'provider = "openai-compat"',
            'policy = "auto-approve"',
            f"working_dir = {toml_str(state)}",
            "",
            "[provider]",
            'base_url = "http://127.0.0.1:1/v1"',
            'model = "unused"',
            'api_key = "none"',
            "",
            "[tokenizer]",
            'backend = "approx"',
            "",
            "[search]",
            "auto_index = false",
            "",
            "[metrics]",
            "enabled = false",
            "",
            "[memory]",
            'backend = "file"',
            f"episodic_path = {toml_str(state / 'episodic.jsonl')}",
            f"semantic_dir = {toml_str(state / 'memory')}",
            "",
            "[grpc.tls]",
            f"cert = {toml_str(cert)}",
            f"key = {toml_str(key)}",
            f"client_ca = {toml_str(lay.ca)}",
            "",
            "[auth]",
            'mode = "oidc"',
            f"redirect_uris = [{toml_str(lay.portal_url)}]",
            "",
            "[[auth.issuers]]",
            f"name = {toml_str(ISSUER_NAME)}",
            'profile = "generic"',
            f"issuer = {toml_str(lay.issuer_url)}",
            f"audience = {toml_str(CLIENT_ID)}",
            f"jwks_url = {toml_str(lay.issuer_url + '/jwks')}",
            'tenant_claim = "org"',
            "trust_roles_claim = true",
            f"client_secret = {toml_str('file:' + str(secret_file))}",
            "",
            "[auth.token]",
            f"issuer = {toml_str(TOKEN_ISSUER)}",
            f"audience = {toml_str(TOKEN_AUDIENCE)}",
            "ttl_secs = 300",
            f"signing_key = {toml_str(lay.pki / 'token-signer' / 'key.pem')}",
            'session_store = "file"',
            f"session_path = {toml_str(state / 'auth-sessions')}",
            "",
        ]
    )


def envoy_spec(lay: Layout, otel_port: int) -> dict:
    """The bridge spec for `portal_envoy.py`: one grpc-web listener to the agent."""
    return {
        "listeners": [
            {
                "name": "gateway_grpc_web",
                "port": lay.edge_port,
                "cluster": "agent_gateway",
                "upstream_port": lay.agent_port,
            }
        ],
        "otel_port": otel_port,
        "gateway_port": lay.agent_port,
    }


def envoy_env(lay: Layout, runtime: str, jwks_file: Path) -> dict[str, str]:
    """The S14 knobs for a hardened bridge: jwt_authn on (against the agent's JWKS,
    fetched by us over mTLS), the portal's exact origin, upstream mTLS.

    `XDG_RUNTIME_DIR` is left alone: podman keeps its runtime state there (crun's
    `$XDG_RUNTIME_DIR/crun`), and pointing it at the work dir left a container no
    later podman could stop once the work dir was gone."""
    cert, key = lay.leaf("envoy")
    return {
        "CONTAINER_RUNTIME": runtime,
        "PORTAL_GRPC_WEB_HOST": "127.0.0.1",
        "PORTAL_WEB_ORIGIN": lay.origin,
        "PORTAL_AUTH": "on",
        "PORTAL_JWT_JWKS": str(jwks_file),
        "PORTAL_JWT_ISSUER": TOKEN_ISSUER,
        "PORTAL_JWT_AUDIENCE": TOKEN_AUDIENCE,
        "PORTAL_UPSTREAM_CA": str(lay.ca),
        "PORTAL_UPSTREAM_CERT": str(cert),
        "PORTAL_UPSTREAM_KEY": str(key),
        "PORTAL_UPSTREAM_SNI": "localhost",
    }


def dart_defines(lay: Layout) -> list[str]:
    return [
        f"--dart-define=PORTAL_GRPC_WEB_URL={lay.edge_url}",
        "--dart-define=PORTAL_AUTH=on",
        f"--dart-define=PORTAL_AUTH_ISSUER={ISSUER_NAME}",
    ]


def run_checked(argv: Sequence[str], what: str, **kw) -> subprocess.CompletedProcess:
    p = subprocess.run(list(argv), capture_output=True, text=True, **kw)
    if p.returncode != 0:
        raise HarnessError(f"{what} failed (exit {p.returncode}):\n{(p.stdout + p.stderr)[-1500:]}")
    return p


def build_portal(flutter: str, src: Path, lay: Layout) -> Path:
    """`flutter build web` of a writable copy of the portal, for this run's ports."""
    tree = lay.work / "portal"
    shutil.copytree(src, tree, ignore=shutil.ignore_patterns("build", ".dart_tool"))
    for p in [tree, *tree.rglob("*")]:
        p.chmod(p.stat().st_mode | 0o200)
    run_checked([flutter, "create", "--platforms=web", "--project-name", "agent_portal", "."], "flutter create", cwd=tree)
    run_checked(
        [flutter, "build", "web", "--pwa-strategy=none", *dart_defines(lay)],
        "flutter build web",
        cwd=tree,
        timeout=900,
    )
    return tree / "build" / "web"


def resolve_tool(explicit: str, attr: str, binary: str) -> str:
    """A browser tool: the given path, else the binary-cached nixpkgs build (the
    flake's own chromium would source-build; see nix/portal/default.nix)."""
    if explicit:
        return explicit
    p = run_checked(["nix", "build", "--no-link", "--print-out-paths", f"nixpkgs#{attr}"], f"resolving {attr}")
    return str(Path(p.stdout.strip().splitlines()[-1]) / "bin" / binary)


def spawn(name: str, argv: Sequence[str], log: Path, **kw) -> e2e.Server:
    with log.open("w") as out:
        proc = subprocess.Popen(list(argv), stdout=out, stderr=subprocess.STDOUT, **kw)
    return e2e.Server(name, proc, log)


def wait_until(check: Callable[[], bool], timeout: float, what: str, proc: e2e.Server | None = None) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if proc is not None and proc.proc.poll() is not None:
            raise HarnessError(f"{what}: {proc.name} exited:\n{proc.tail()}")
        try:
            if check():
                return
        except HarnessError:
            pass
        time.sleep(0.5)
    raise HarnessError(f"{what} within {timeout:.0f}s" + (f":\n{proc.tail()}" if proc else ""))


def http_ok(url: str) -> bool:
    try:
        with urllib.request.urlopen(url, timeout=3) as r:
            return r.status == 200
    except (urllib.error.URLError, OSError):
        return False


def rendered_config(name: str, env: Mapping[str, str]) -> Path:
    """Where `portal_envoy.py up` writes the bridge config (0600, it names the JWKS)."""
    return Path(env.get("XDG_RUNTIME_DIR") or "/tmp") / f"{name}-envoy.yaml"


def remove_container(runtime: str, name: str) -> None:
    """Remove this run's bridge and its rendered config. A container that will not
    go is reported, never passed over in silence: the next run could not reuse the
    name."""
    p = subprocess.run([runtime, "rm", "-f", "-t", "5", name], capture_output=True, text=True)
    if p.returncode != 0:
        print(
            f"portal-auth-e2e: [warn] could not remove container {name}: "
            f"{(p.stderr or p.stdout).strip()[:300]}",
            file=sys.stderr,
        )
    rendered_config(name, os.environ).unlink(missing_ok=True)


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    p = argparse.ArgumentParser(prog="portal-auth-e2e", description=__doc__.split("\n\n")[0])
    p.add_argument("--agent", default="agent")
    p.add_argument("--grpcurl", default="grpcurl")
    p.add_argument("--pki-dev", default="pki-dev", help="the pki-dev command (split on spaces)")
    p.add_argument("--flutter", default="flutter")
    p.add_argument("--static-web-server", default="static-web-server")
    p.add_argument("--portal-src", default="portal", help="the portal source tree to build")
    p.add_argument("--web-bundle", default="", help="a prebuilt bundle for these ports (skips the build)")
    p.add_argument("--chromium", default=os.environ.get("PORTAL_E2E_CHROMIUM", ""))
    p.add_argument("--chromedriver", default=os.environ.get("PORTAL_E2E_CHROMEDRIVER", ""))
    p.add_argument("--portal-envoy", required=True, help="test/portal-envoy/portal_envoy.py")
    p.add_argument("--proto-dir", required=True)
    p.add_argument("--envoy-image", required=True)
    p.add_argument("--envoy-name", required=True)
    p.add_argument("--edge-port", type=int, required=True)
    p.add_argument("--web-port", type=int, required=True)
    p.add_argument("--runtime", default="docker")
    p.add_argument("--keep", action="store_true", help="keep the work directory")
    return p.parse_args(argv)


def runtime_up(runtime: str) -> bool:
    try:
        return subprocess.run([runtime, "info"], capture_output=True, timeout=30).returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def main(argv: Sequence[str]) -> int:
    args = parse_args(argv)
    if args.runtime not in ("docker", "podman"):
        print(f"FAIL(harness): CONTAINER_RUNTIME must be docker or podman, not {args.runtime!r}", file=sys.stderr)
        return EXIT_HARNESS
    if not runtime_up(args.runtime):
        print(
            f"portal-auth-e2e: SKIP — container runtime '{args.runtime}' not reachable "
            "(the hardened Envoy runs as a container; CONTAINER_RUNTIME=podman on podman hosts)"
        )
        return EXIT_OK
    work = Path(tempfile.mkdtemp(prefix="portal-auth-e2e-"))
    secret = secrets.token_urlsafe(24)
    procs: list[e2e.Server] = []
    browser: Browser | None = None
    issuer: CodeIssuer | None = None
    envoy_started = False
    try:
        agent_port = e2e.free_port()
        lay = Layout(work, "", agent_port, args.edge_port, args.web_port)
        issuer = CodeIssuer(secret, [lay.portal_url])
        issuer.start()
        lay = Layout(work, issuer.url, agent_port, args.edge_port, args.web_port)
        (work / "agent").mkdir()
        secret_file = work / "idp-client-secret"
        secret_file.write_text(secret)
        secret_file.chmod(0o600)
        pki = [*args.pki_dev.split(), "--out", str(lay.pki), "--deployment", e2e.DEPLOYMENT]
        run_checked([*pki, "--service", "agent", "--service", "envoy", "--service", "cli"], "pki-dev")

        chromium = resolve_tool(args.chromium, "chromium", "chromium")
        chromedriver = resolve_tool(args.chromedriver, "chromedriver", "chromedriver")
        bundle = Path(args.web_bundle) if args.web_bundle else None
        if bundle is None:
            print("portal-auth-e2e: building the portal web bundle ...")
            bundle = build_portal(args.flutter, Path(args.portal_src), lay)

        cfg = work / "agent.toml"
        cfg.write_text(render_config(lay, secret_file))
        cli_cert, cli_key = lay.leaf("cli")
        user = Client(args.grpcurl, lay.ca, cli_cert, cli_key)
        agent = spawn(
            "agent",
            [args.agent, "--config", str(cfg), "--serve-all", "--listen", f"https://{lay.agent_addr}"],
            work / "agent.log",
            cwd=work / "agent",
        )
        procs.append(agent)
        wait_until(
            lambda: user.call(lay.agent_addr, "grpc.health.v1.Health/Check").code == "OK",
            30,
            "the agent never became healthy",
            agent,
        )
        jwks = expect(user.call(lay.agent_addr, "agent.v1.AuthService/Jwks"), "OK", "AuthService.Jwks")
        jwks_file = work / "agent-jwks.json"
        jwks_file.write_text(str(jwks.body.get("jwksJson", "")))
        spec_file = work / "envoy-spec.json"
        spec_file.write_text(json.dumps(envoy_spec(lay, e2e.free_port())))

        print(f"portal-auth-e2e: starting the hardened bridge ({args.envoy_name}) ...")
        envoy_started = True
        run_checked(
            [
                sys.executable, args.portal_envoy, "--spec", str(spec_file),
                "--grpcurl", args.grpcurl, "--proto-dir", args.proto_dir,
                "up", "--name", args.envoy_name, "--image", args.envoy_image,
            ],
            "portal_envoy.py up",
            env={**os.environ, **envoy_env(lay, args.runtime, jwks_file)},
            timeout=EDGE_WAIT,
        )
        wait_until(
            lambda: call_edge(lay.edge_url, lay.origin, HEALTH_RPC, None).passed,
            EDGE_WAIT,
            "the bridge never passed a health check to the agent",
        )
        web = spawn(
            "static-web-server",
            [
                args.static_web_server, "--root", str(bundle), "--host", "127.0.0.1",
                "--port", str(lay.web_port), "--cache-control-headers=false",
            ],
            work / "web.log",
        )
        procs.append(web)
        wait_until(lambda: http_ok(lay.portal_url), 30, "the portal was never served", web)

        driver_port = e2e.free_port()
        driver = spawn("chromedriver", [chromedriver, f"--port={driver_port}"], work / "chromedriver.log")
        procs.append(driver)
        wd = f"http://127.0.0.1:{driver_port}"
        wait_until(lambda: http_ok(wd + "/status"), 30, "chromedriver never answered", driver)
        browser = Browser(http_transport(wd), chromium)

        ctx = Ctx(
            browser=browser,
            flow=issuer.flow,
            edge=lambda path, bearer, session=None: call_edge(lay.edge_url, lay.origin, path, bearer, session),
            agent=lambda method, data=None, bearer=None: user.call(lay.agent_addr, method, data, bearer=bearer),
            portal_url=lay.portal_url,
        )
        code = e2e.run_steps(ctx, STEPS)
        if code != EXIT_OK:
            print(f"--- agent log tail\n{agent.tail()}", file=sys.stderr)
        return code
    except HarnessError as e:
        print(f"FAIL(harness): {e}", file=sys.stderr)
        return EXIT_HARNESS
    finally:
        if browser is not None:
            browser.close()
        for p in reversed(procs):
            p.stop()
        if envoy_started:
            remove_container(args.runtime, args.envoy_name)
        if issuer is not None:
            issuer.stop()
        if args.keep:
            print(f"work dir kept: {work}", file=sys.stderr)
        else:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
