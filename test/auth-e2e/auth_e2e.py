"""auth-e2e: the whole sign-in chain over real processes (security-hardening S15a).

Stands up, on loopback only:

- a fake OIDC login issuer (discovery + JWKS over plain http, ES256 ID tokens
  signed with a key made at start-up);
- an offline dev PKI (`pki-dev`, step-cli): CA, per-service leaves and the
  agent's token-signer key;
- server B: `agent --serve-memory` over mTLS, the memory store;
- server A: `agent --serve-all` over mTLS, `mode = "oidc"` with `[auth.token]`,
  whose memory is a `= "grpc"` client of B,

then drives both with grpcurl and checks the contract of
docs/design/security-hardening/04-service-integration.md, "Process wire":

- login tokens are accepted only by `AuthService.Exchange`, and only when they
  verify (signature, `iss`, `aud`, expiry);
- agent tokens carry the tenant, which beats any `x-agent-user-id` header;
- a user's memory write through A lands in B under that user's tenant (the
  bearer is forwarded across the `= "grpc"` hop) and another tenant can't see it;
- service identity: a bound client certificate exchanges for a `svc:` token, an
  unbound one is refused, and a listener without a client certificate refuses
  the handshake;
- refresh rotates the handle, a replayed handle is refused, and a refresh after
  logout is refused;
- a forwarding loop (`x-agent-hops` over the ceiling) is refused.

Exit codes follow nix/lib/contract.sh: 0 ok, 1 harness (the setup broke), 2
contract (the agent answered wrongly).
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Callable, Mapping, Sequence

import jwt
from cryptography.hazmat.primitives.asymmetric import ec

EXIT_OK, EXIT_HARNESS, EXIT_CONTRACT = 0, 1, 2

# The login audience (the IdP's client id) and the agent's own token claims.
LOGIN_AUDIENCE = "agent-e2e-login"
TOKEN_ISSUER = "https://agent.e2e.invalid"
TOKEN_AUDIENCE = "agent-e2e"
DEPLOYMENT = "e2e"
TENANT_A = "tenant-a"
TENANT_B = "tenant-b"


class HarnessError(Exception):
    """The environment broke (a server never came up, a tool is missing)."""


class ContractError(Exception):
    """The agent answered, but wrongly."""


# ---------------------------------------------------------------- grpcurl


@dataclass(frozen=True)
class Reply:
    """One grpcurl call: `code` is the gRPC status name (`OK` on success) or
    `DIAL` when no call was made (connection or TLS handshake refused)."""

    code: str
    body: dict
    raw: str


_CODE_RE = re.compile(r"^\s*Code:\s*([A-Za-z]+)\s*$", re.MULTILINE)
_DIAL_MARKERS = ("Failed to dial target host", "context deadline exceeded", "connection refused")


def parse_reply(returncode: int, stdout: str, stderr: str) -> Reply:
    """Classify grpcurl's output. A zero exit is `OK` with the JSON body (empty
    object for an empty reply); a `Code:` line gives the status; a dial failure is
    `DIAL`. Anything else is a harness error, never a guess."""
    if returncode == 0:
        text = stdout.strip()
        if not text:
            return Reply("OK", {}, stdout)
        try:
            body = json.loads(text)
        except json.JSONDecodeError as e:
            raise HarnessError(f"grpcurl printed non-JSON on success: {text[:200]!r}") from e
        if not isinstance(body, dict):
            raise HarnessError(f"grpcurl printed a non-object reply: {text[:200]!r}")
        return Reply("OK", body, stdout)
    text = stdout + stderr
    m = _CODE_RE.search(text)
    if m:
        return Reply(m.group(1), {}, text)
    if any(marker in text for marker in _DIAL_MARKERS):
        return Reply("DIAL", {}, text)
    raise HarnessError(f"grpcurl failed without a status (exit {returncode}): {text[-400:]!r}")


@dataclass(frozen=True)
class Client:
    """A grpcurl caller bound to one TLS identity (`cert` None ⇒ no client cert)."""

    grpcurl: str
    ca: Path
    cert: Path | None = None
    key: Path | None = None
    runner: Callable[..., subprocess.CompletedProcess] = subprocess.run

    def call(
        self,
        addr: str,
        method: str,
        data: Mapping | None = None,
        *,
        bearer: str | None = None,
        headers: Mapping[str, str] | None = None,
    ) -> Reply:
        argv = [self.grpcurl, "-max-time", "10", "-cacert", str(self.ca), "-servername", "localhost"]
        if self.cert is not None and self.key is not None:
            argv += ["-cert", str(self.cert), "-key", str(self.key)]
        if bearer is not None:
            argv += ["-H", f"authorization: Bearer {bearer}"]
        for k, v in (headers or {}).items():
            argv += ["-H", f"{k}: {v}"]
        argv += ["-d", json.dumps(dict(data or {})), addr, method]
        p = self.runner(argv, capture_output=True, text=True, timeout=30)
        return parse_reply(p.returncode, p.stdout, p.stderr)


# ---------------------------------------------------------------- the fake issuer


class Issuer:
    """A fake OIDC login issuer: discovery and a JWK set over loopback http, and
    ID tokens signed with a P-256 key generated here (never persisted)."""

    def __init__(self) -> None:
        self.key = ec.generate_private_key(ec.SECP256R1())
        self.kid = uuid.uuid4().hex[:16]
        self._server = ThreadingHTTPServer(("127.0.0.1", 0), self._handler())
        self.url = f"http://127.0.0.1:{self._server.server_address[1]}"
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)

    def jwks(self) -> dict:
        jwk = json.loads(jwt.algorithms.ECAlgorithm.to_jwk(self.key.public_key()))
        jwk.update({"kid": self.kid, "alg": "ES256", "use": "sig"})
        return {"keys": [jwk]}

    def discovery(self) -> dict:
        return {
            "issuer": self.url,
            "jwks_uri": f"{self.url}/jwks",
            "id_token_signing_alg_values_supported": ["ES256"],
        }

    def _handler(self) -> type[BaseHTTPRequestHandler]:
        issuer = self

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:  # noqa: N802 (http.server's name)
                routes = {
                    "/.well-known/openid-configuration": issuer.discovery,
                    "/jwks": issuer.jwks,
                }
                page = routes.get(self.path)
                if page is None:
                    self.send_error(404)
                    return
                body = json.dumps(page()).encode()
                self.send_response(200)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *_args) -> None:
                pass

        return Handler

    def start(self) -> None:
        self._thread.start()

    def stop(self) -> None:
        self._server.shutdown()
        self._server.server_close()

    def mint(self, claims: Mapping, *, key: ec.EllipticCurvePrivateKey | None = None) -> str:
        return jwt.encode(dict(claims), key or self.key, algorithm="ES256", headers={"kid": self.kid})


def login_claims(
    issuer: str,
    subject: str,
    tenant: str,
    *,
    roles: Sequence[str] = ("org_admin",),
    now: int | None = None,
    ttl: int = 600,
    audience: str = LOGIN_AUDIENCE,
) -> dict:
    """The claims of a login ID token from the fake issuer."""
    now = int(time.time()) if now is None else now
    return {
        "iss": issuer,
        "aud": audience,
        "sub": subject,
        "email": f"{subject}@{tenant}.example",
        "email_verified": True,
        "org": tenant,
        "roles": list(roles),
        "iat": now,
        "nbf": now - 5,
        "exp": now + ttl,
    }


# ---------------------------------------------------------------- config


def toml_str(value: str | Path) -> str:
    """A TOML basic string. JSON string escapes are a subset of TOML's, so the
    value can never close the string or inject a key."""
    return json.dumps(str(value))


@dataclass(frozen=True)
class Layout:
    work: Path
    pki: Path
    issuer_url: str
    port_a: int
    port_b: int

    def leaf(self, service: str) -> tuple[Path, Path]:
        return self.pki / service / "cert.pem", self.pki / service / "key.pem"

    @property
    def ca(self) -> Path:
        return self.pki / "ca" / "root.crt"

    @property
    def addr_a(self) -> str:
        return f"127.0.0.1:{self.port_a}"

    @property
    def addr_b(self) -> str:
        return f"127.0.0.1:{self.port_b}"


def spiffe(service: str) -> str:
    return f"spiffe://agent.{DEPLOYMENT}/svc/{service}"


def render_config(lay: Layout, server: str) -> str:
    """`agent.toml` for server `a` (serve-all, memory = grpc → B) or `b`
    (serve-memory, file store). Both verify the same agent tokens: one cluster,
    one token signer."""
    if server not in ("a", "b"):
        raise ValueError(f"unknown server {server!r}")
    own_cert, own_key = lay.leaf("agent" if server == "a" else "memory")
    state = lay.work / server
    lines = [
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
    ]
    if server == "a":
        lines += ['backend = "grpc"', ""]
    else:
        lines += [
            'backend = "file"',
            f"episodic_path = {toml_str(state / 'episodic.jsonl')}",
            f"semantic_dir = {toml_str(state / 'memory')}",
            "",
        ]
    lines += [
        "[grpc.tls]",
        f"cert = {toml_str(own_cert)}",
        f"key = {toml_str(own_key)}",
        f"client_ca = {toml_str(lay.ca)}",
        "",
    ]
    if server == "a":
        lines += [
            "[grpc.tls.client]",
            f"ca = {toml_str(lay.ca)}",
            f"cert = {toml_str(own_cert)}",
            f"key = {toml_str(own_key)}",
            'domain = "localhost"',
            "",
            "[grpc.memory]",
            f"endpoint = {toml_str('https://' + lay.addr_b)}",
            "",
        ]
    lines += [
        "[auth]",
        'mode = "oidc"',
        "",
        "[[auth.issuers]]",
        'name = "fake"',
        'profile = "generic"',
        f"issuer = {toml_str(lay.issuer_url)}",
        f"audience = {toml_str(LOGIN_AUDIENCE)}",
        f"jwks_url = {toml_str(lay.issuer_url + '/jwks')}",
        'tenant_claim = "org"',
        "trust_roles_claim = true",
        "",
        "[auth.token]",
        f"issuer = {toml_str(TOKEN_ISSUER)}",
        f"audience = {toml_str(TOKEN_AUDIENCE)}",
        "ttl_secs = 300",
        f"signing_key = {toml_str(lay.pki / 'token-signer' / 'key.pem')}",
        'session_store = "file"',
        f"session_path = {toml_str(state / 'auth-sessions')}",
        "",
        "[[auth.mtls.bindings]]",
        f"san = {toml_str(spiffe('fleet'))}",
        'service = "fleet"',
        f"tenant = {toml_str(TENANT_A)}",
        'roles = ["svc_fleet"]',
        "",
    ]
    return "\n".join(lines)


# ---------------------------------------------------------------- processes


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


@dataclass
class Server:
    name: str
    proc: subprocess.Popen
    log: Path

    def tail(self, n: int = 40) -> str:
        try:
            return "\n".join(self.log.read_text(errors="replace").splitlines()[-n:])
        except OSError:
            return "(no log)"

    def stop(self) -> None:
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()


def start_server(agent: str, lay: Layout, name: str, flags: Sequence[str], health: Client, addr: str) -> Server:
    """Boot one agent and wait (≤ 30 s) for `grpc.health.v1` SERVING over mTLS."""
    cfg = lay.work / f"agent.{name}.toml"
    cfg.write_text(render_config(lay, name))
    state = lay.work / name
    state.mkdir(exist_ok=True)
    log = lay.work / f"server.{name}.log"
    with log.open("w") as out:
        proc = subprocess.Popen(
            [agent, "--config", str(cfg), *flags, "--listen", f"https://{addr}"],
            stdout=out,
            stderr=subprocess.STDOUT,
            cwd=state,  # each server indexes its own directory
        )
    srv = Server(name, proc, log)
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise HarnessError(f"server {name} exited during start-up:\n{srv.tail()}")
        try:
            if health.call(addr, "grpc.health.v1.Health/Check").code == "OK":
                return srv
        except HarnessError:
            pass
        time.sleep(0.2)
    srv.stop()
    raise HarnessError(f"server {name} never became healthy:\n{srv.tail()}")


# ---------------------------------------------------------------- the contract


@dataclass
class Ctx:
    lay: Layout
    issuer: Issuer
    user: Client  # the `cli` leaf: a person's client (portal via Envoy, CLI)
    fleet: Client  # the `fleet` leaf: bound to svc:fleet in tenant A
    bare: Client  # no client certificate
    tokens: dict = field(default_factory=dict)


def expect(reply: Reply, code: str, what: str) -> Reply:
    if reply.code != code:
        raise ContractError(f"{what}: want {code}, got {reply.code}: {reply.raw.strip()[:300]}")
    return reply


def scoped(session: str) -> dict:
    return {"x-agent-session-id": session}


def exchange(ctx: Ctx, id_token: str, what: str, code: str = "OK") -> Reply:
    return expect(
        ctx.user.call(ctx.lay.addr_a, "agent.v1.AuthService/Exchange", {"idToken": id_token, "clientKind": "cli"}),
        code,
        what,
    )


def step_health_is_exempt(ctx: Ctx) -> None:
    for addr in (ctx.lay.addr_a, ctx.lay.addr_b):
        expect(ctx.user.call(addr, "grpc.health.v1.Health/Check"), "OK", f"health on {addr} without a token")


def step_no_bearer_refused(ctx: Ctx) -> None:
    expect(
        ctx.user.call(ctx.lay.addr_a, "agent.v1.Memory/Recall", {"text": "x"}, headers=scoped("s")),
        "Unauthenticated",
        "Recall without a bearer",
    )
    expect(ctx.user.call(ctx.lay.addr_a, "agent.v1.AuthService/WhoAmI"), "Unauthenticated", "WhoAmI without a bearer")


def step_login_tokens_verified(ctx: Ctx) -> None:
    iss = ctx.issuer
    now = int(time.time())
    stranger = ec.generate_private_key(ec.SECP256R1())
    bad = {
        "a key the issuer never published": iss.mint(login_claims(iss.url, "mallory", TENANT_A), key=stranger),
        "an expired login token": iss.mint(login_claims(iss.url, "alice", TENANT_A, now=now - 7200, ttl=60)),
        "another client's audience": iss.mint(login_claims(iss.url, "alice", TENANT_A, audience="someone-else")),
        "an unknown issuer": iss.mint(login_claims("http://127.0.0.1:1", "alice", TENANT_A)),
        "an unsigned token": jwt.encode(login_claims(iss.url, "alice", TENANT_A), None, algorithm="none"),
        "garbage": "not.a.jwt",
    }
    for what, token in bad.items():
        exchange(ctx, token, f"Exchange with {what}", code="Unauthenticated")


def step_exchange_mints_agent_tokens(ctx: Ctx) -> None:
    iss = ctx.issuer
    for name, tenant in (("alice", TENANT_A), ("bob", TENANT_B)):
        reply = exchange(ctx, iss.mint(login_claims(iss.url, name, tenant)), f"Exchange for {name}")
        token, handle = reply.body.get("accessToken", ""), reply.body.get("refreshHandle", "")
        if not token or not handle:
            raise ContractError(f"Exchange for {name} returned no token or refresh handle: {reply.body}")
        claims = jwt.decode(token, options={"verify_signature": False})
        if claims.get("iss") != TOKEN_ISSUER or claims.get("aud") not in (TOKEN_AUDIENCE, [TOKEN_AUDIENCE]):
            raise ContractError(f"agent token for {name} has iss/aud {claims.get('iss')}/{claims.get('aud')}")
        ctx.tokens[name] = {"access": token, "refresh": handle, "login": iss.mint(login_claims(iss.url, name, tenant))}


def step_login_token_refused_at_seams(ctx: Ctx) -> None:
    login = ctx.tokens["alice"]["login"]
    expect(
        ctx.user.call(ctx.lay.addr_a, "agent.v1.AuthService/WhoAmI", bearer=login),
        "Unauthenticated",
        "WhoAmI with a login token (only Exchange takes one)",
    )
    expect(
        ctx.user.call(ctx.lay.addr_a, "agent.v1.Memory/Recall", {"text": "x"}, bearer=login, headers=scoped("s")),
        "Unauthenticated",
        "Recall with a login token",
    )


def step_whoami_tenant_beats_header(ctx: Ctx) -> None:
    for name, tenant in (("alice", TENANT_A), ("bob", TENANT_B)):
        reply = expect(
            ctx.user.call(
                ctx.lay.addr_a,
                "agent.v1.AuthService/WhoAmI",
                bearer=ctx.tokens[name]["access"],
                headers={"x-agent-user-id": "someone-else"},
            ),
            "OK",
            f"WhoAmI for {name}",
        )
        if reply.body.get("tenant") != tenant or not reply.body.get("sid"):
            raise ContractError(f"WhoAmI for {name}: want tenant {tenant} and a sid, got {reply.body}")


def _remember(ctx: Ctx, addr: str, who: str, text: str, *, spoof: str | None = None) -> Reply:
    headers = scoped(f"{who}-s1")
    if spoof is not None:
        headers["x-agent-user-id"] = spoof
    event = {"kind": "user", "message": {"role": "ROLE_USER", "content": text}, "sessionId": f"{who}-s1"}
    return ctx.user.call(addr, "agent.v1.Memory/Append", event, bearer=ctx.tokens[who]["access"], headers=headers)


def _recall(ctx: Ctx, addr: str, who: str, text: str) -> list[str]:
    reply = expect(
        ctx.user.call(
            addr,
            "agent.v1.Memory/Recall",
            {"text": text, "limit": 10},
            bearer=ctx.tokens[who]["access"],
            headers=scoped(f"{who}-s1"),
        ),
        "OK",
        f"Recall by {who} at {addr}",
    )
    return [item.get("content", "") for item in reply.body.get("items", [])]


def stored_in(root: Path, marker: str) -> list[str]:
    """Which top-level directories of a file memory store hold `marker` — the
    tenant partitions of `--serve-memory`'s file backend (`<root>/<tenant>/…`),
    `.` for the unpartitioned root."""
    hits = set()
    for path in root.rglob("*.jsonl"):
        try:
            if marker in path.read_text(errors="replace"):
                rel = path.relative_to(root).parts
                hits.add(rel[0] if len(rel) > 1 else ".")
        except OSError:
            continue
    return sorted(hits)


def step_chain_forwards_tenant(ctx: Ctx) -> None:
    marker = f"e2e-marker-{uuid.uuid4().hex[:12]}"
    # Alice writes through A while claiming tenant B in the header: the verified
    # tenant must win at A, and again at B after the `= "grpc"` hop, so the event
    # lands in B's tenant-A partition and nowhere else.
    expect(_remember(ctx, ctx.lay.addr_a, "alice", marker, spoof=TENANT_B), "OK", "Append by alice via A")
    where = stored_in(ctx.lay.work / "b", marker)
    if where != [TENANT_A]:
        raise ContractError(f"alice's write via A landed in B's {where}, want [{TENANT_A!r}] (bearer not forwarded?)")
    where = stored_in(ctx.lay.work / "a", marker)
    if where:
        raise ContractError(f"A kept a local copy of the event in {where}; its memory is B")
    # Bob reads through A and at B directly: nothing of alice's comes back.
    for addr in (ctx.lay.addr_a, ctx.lay.addr_b):
        if any(marker in c for c in _recall(ctx, addr, "bob", marker)):
            raise ContractError(f"bob (tenant B) can read alice's memory at {addr}")


def step_hops_ceiling(ctx: Ctx) -> None:
    expect(
        ctx.user.call(
            ctx.lay.addr_b,
            "agent.v1.Memory/Recall",
            {"text": "x"},
            bearer=ctx.tokens["alice"]["access"],
            headers={**scoped("alice-s1"), "x-agent-hops": "9"},
        ),
        "FailedPrecondition",
        "a call claiming 9 forwarding hops",
    )


def step_mtls_service_identity(ctx: Ctx) -> None:
    exchange_svc = "agent.v1.AuthService/Exchange"
    reply = expect(ctx.fleet.call(ctx.lay.addr_a, exchange_svc, {"useClientCert": True}), "OK", "service Exchange (fleet)")
    token = reply.body.get("accessToken", "")
    who = expect(ctx.fleet.call(ctx.lay.addr_a, "agent.v1.AuthService/WhoAmI", bearer=token), "OK", "WhoAmI as svc:fleet")
    if who.body.get("tenant") != TENANT_A or not str(who.body.get("subject", "")).startswith("svc:"):
        raise ContractError(f"service token: want tenant {TENANT_A} and a svc: subject, got {who.body}")
    # The token is bound to the fleet certificate (`cnf`): the same bearer over
    # another client's certificate is refused.
    expect(
        ctx.user.call(ctx.lay.addr_a, "agent.v1.AuthService/WhoAmI", bearer=token),
        "Unauthenticated",
        "a svc:fleet token presented over the cli certificate",
    )
    expect(
        ctx.user.call(ctx.lay.addr_a, exchange_svc, {"useClientCert": True}),
        "Unauthenticated",
        "service Exchange with an unbound certificate",
    )
    reply = ctx.bare.call(ctx.lay.addr_a, "grpc.health.v1.Health/Check")
    if reply.code == "OK":
        raise ContractError("an mTLS listener answered a client with no certificate")


def step_refresh_and_logout(ctx: Ctx) -> None:
    refresh = "agent.v1.AuthService/Refresh"
    first = ctx.tokens["bob"]["refresh"]
    reply = expect(ctx.user.call(ctx.lay.addr_a, refresh, {"refreshHandle": first}), "OK", "Refresh for bob")
    second = reply.body.get("refreshHandle", "")
    if not second or second == first or not reply.body.get("accessToken"):
        raise ContractError(f"Refresh did not rotate the handle: {reply.body}")
    expect(
        ctx.user.call(ctx.lay.addr_a, refresh, {"refreshHandle": first}),
        "Unauthenticated",
        "a replayed refresh handle",
    )
    # A replay revokes the session, so the rotated handle is dead too.
    expect(
        ctx.user.call(ctx.lay.addr_a, refresh, {"refreshHandle": second}),
        "Unauthenticated",
        "the rotated handle after a replay",
    )
    alice = ctx.tokens["alice"]
    expect(
        ctx.user.call(ctx.lay.addr_a, "agent.v1.AuthService/Logout", bearer=alice["access"]),
        "OK",
        "Logout for alice",
    )
    expect(
        ctx.user.call(ctx.lay.addr_a, refresh, {"refreshHandle": alice["refresh"]}),
        "Unauthenticated",
        "Refresh after Logout",
    )


STEPS: tuple[tuple[str, Callable[[Ctx], None]], ...] = (
    ("health is exempt", step_health_is_exempt),
    ("no bearer is refused", step_no_bearer_refused),
    ("bad login tokens are refused", step_login_tokens_verified),
    ("Exchange mints agent tokens", step_exchange_mints_agent_tokens),
    ("login tokens are refused at seams", step_login_token_refused_at_seams),
    ("verified tenant beats the header", step_whoami_tenant_beats_header),
    ("chain forwards the tenant across a grpc seam", step_chain_forwards_tenant),
    ("forwarding loops are refused", step_hops_ceiling),
    ("mTLS service identity", step_mtls_service_identity),
    ("refresh rotation and logout", step_refresh_and_logout),
)


def run_steps(ctx: Ctx, steps: Sequence[tuple[str, Callable[[Ctx], None]]], out=sys.stdout) -> int:
    """Run every step in order; the first failure stops the run (later steps
    depend on earlier state). Returns the contract exit code."""
    for name, fn in steps:
        try:
            fn(ctx)
        except ContractError as e:
            print(f"FAIL {name}: {e}", file=out)
            return EXIT_CONTRACT
        print(f"ok   {name}", file=out)
    return EXIT_OK


# ---------------------------------------------------------------- main


def mint_pki(pki_dev: Sequence[str], out: Path) -> None:
    argv = [*pki_dev, "--out", str(out), "--deployment", DEPLOYMENT]
    for svc in ("agent", "memory", "fleet", "cli"):
        argv += ["--service", svc]
    p = subprocess.run(argv, capture_output=True, text=True)
    if p.returncode != 0:
        raise HarnessError(f"pki-dev failed:\n{p.stdout}{p.stderr}")


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    p = argparse.ArgumentParser(prog="auth-e2e", description=__doc__.split("\n\n")[0])
    p.add_argument("--agent", default="agent", help="the agent binary")
    p.add_argument("--grpcurl", default="grpcurl", help="the grpcurl binary")
    p.add_argument("--pki-dev", default="pki-dev", help="the pki-dev command (split on spaces)")
    p.add_argument("--keep", action="store_true", help="keep the work directory")
    return p.parse_args(argv)


def main(argv: Sequence[str]) -> int:
    args = parse_args(argv)
    for tool in (args.agent, args.grpcurl):
        if shutil.which(tool) is None:
            print(f"FAIL(harness): {tool} not found", file=sys.stderr)
            return EXIT_HARNESS
    work = Path(tempfile.mkdtemp(prefix="auth-e2e-"))
    issuer = Issuer()
    servers: list[Server] = []
    try:
        issuer.start()
        lay = Layout(work, work / "pki", issuer.url, free_port(), free_port())
        mint_pki(args.pki_dev.split(), lay.pki)
        cli_cert, cli_key = lay.leaf("cli")
        fleet_cert, fleet_key = lay.leaf("fleet")
        ctx = Ctx(
            lay=lay,
            issuer=issuer,
            user=Client(args.grpcurl, lay.ca, cli_cert, cli_key),
            fleet=Client(args.grpcurl, lay.ca, fleet_cert, fleet_key),
            bare=Client(args.grpcurl, lay.ca),
        )
        servers.append(start_server(args.agent, lay, "b", ["--serve-memory"], ctx.user, lay.addr_b))
        servers.append(start_server(args.agent, lay, "a", ["--serve-all"], ctx.user, lay.addr_a))
        code = run_steps(ctx, STEPS)
        if code != EXIT_OK:
            for srv in servers:
                print(f"--- server {srv.name} log tail\n{srv.tail()}", file=sys.stderr)
        return code
    except HarnessError as e:
        print(f"FAIL(harness): {e}", file=sys.stderr)
        return EXIT_HARNESS
    finally:
        for srv in reversed(servers):
            srv.stop()
        issuer.stop()
        if args.keep:
            print(f"work dir kept: {work}", file=sys.stderr)
        else:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
