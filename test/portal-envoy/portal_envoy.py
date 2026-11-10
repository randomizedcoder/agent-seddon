#!/usr/bin/env python3
"""The portal's Envoy grpc-web bridge: render its config and bring it up.

`nix run .#grpc-web-up` is a shim over `portal_envoy.py up`; the `portal-envoy`
check runs `render` and `envoy --mode validate` on the result. Envoy accepts JSON
as YAML, so the config is built as a dict and written with `json.dumps`: no value
from the environment can break out of the string it lands in.

Knobs (environment; the defaults are the safe ones):

    PORTAL_GRPC_WEB_HOST   listener bind for every listener       127.0.0.1
    PORTAL_WEB_ORIGIN      comma list of exact CORS origins       http://127.0.0.1:8092,
                                                                  http://localhost:8092
    PORTAL_AUTH            auto | on | off                        auto
                           auto: ask the agent for its JWKS; if it serves no agent
                           tokens, render no jwt_authn (the agent has nothing to
                           check). on: jwt_authn or refuse to start. off: never.
    PORTAL_JWT_JWKS        a JWKS file, or an https (loopback http) URL; unset ⇒
                           fetched from PORTAL_JWKS_FROM with AuthService.Jwks
    PORTAL_JWKS_FROM       host:port of the agent to fetch from   127.0.0.1:<gateway>
    PORTAL_JWKS_WAIT       seconds to wait for that agent         30
    PORTAL_JWT_ISSUER      expected `iss` (unset: the agent alone checks it)
    PORTAL_JWT_AUDIENCE    comma list of accepted `aud` (same)
    PORTAL_TLS_CERT/_KEY   serve every listener over TLS (both or neither)
    PORTAL_UPSTREAM_CA     dial the agent over TLS, verifying against this CA
    PORTAL_UPSTREAM_CERT/_KEY  and present this client certificate (mTLS)
    PORTAL_UPSTREAM_SNI    SNI + the DNS SAN the agent's certificate must carry
                                                                  localhost
    PORTAL_OTLP_AUTHORIZATION | CLICKSTACK_INGESTION_API_KEY  OTLP ingestion key
    PORTAL_FRONT_DOOR      off | on: the front door (:8443, :8444) off
                           every request checked by the agent's gate (ext_authz)
    PORTAL_FRONT_DOOR_CERT/_KEY  its TLS certificate (needed when on)
    PORTAL_FRONT_DOOR_HOST       its bind                         127.0.0.1
    PORTAL_FRONT_DOOR_ORIGINS    comma list of other origins allowed CORS (none)

Design: docs/design/security-hardening/06-portal-and-edge.md, and
10-sso-front-door.md for the front door.
"""

from __future__ import annotations

import argparse
import ipaddress
import json
import os
import re
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Mapping, Sequence
from urllib.parse import urlsplit

AUTH_MODES = ("auto", "on", "off")
DEFAULT_ORIGINS = ("http://127.0.0.1:8092", "http://localhost:8092")
MAX_ORIGINS = 16
MAX_JWKS_BYTES = 64 * 1024
DEFAULT_JWKS_WAIT = 30
MAX_JWKS_WAIT = 600
PROVIDER = "agent"
JWKS_CLUSTER = "agent_jwks"
# Paths the agent answers without a bearer (docs/grpc.md): signing in, health, reflection.
UNAUTHENTICATED_PREFIXES = (
    "/agent.v1.AuthService/",
    "/grpc.health.v1.Health/",
    "/grpc.reflection.",
)
ALLOW_HEADERS = (
    "keep-alive,user-agent,cache-control,content-type,content-transfer-encoding,"
    "x-grpc-web,x-user-agent,grpc-timeout,authorization,x-agent-user-id,"
    "x-agent-session-id,traceparent,tracestate,x-request-id"
)
EXPOSE_HEADERS = "grpc-status,grpc-message,x-envoy-upstream-service-time,traceparent,tracestate"
# Where `up` mounts the TLS files inside the container.
CONTAINER_TLS_DIR = "/etc/envoy/tls"


class EnvoyError(Exception):
    """A configuration or bring-up error, reported without a traceback."""


@dataclass(frozen=True)
class Listener:
    name: str
    port: int
    cluster: str
    upstream_port: int


@dataclass(frozen=True)
class Rest:
    """The REST/JSON transcoder listener (rest-openapi §4): a `grpc_json_transcoder`
    chain over the agent_gateway cluster, projecting the gRPC surface to REST per the
    `.proto` `(google.api.http)` routes. Loopback-pinned (compat surface; the agent's
    own AuthLayer still applies), so it does not honour PORTAL_GRPC_WEB_HOST."""

    name: str
    port: int
    cluster: str
    upstream_port: int
    descriptor: str  # the FileDescriptorSet path (nix/rest-descriptor.nix)
    services: tuple[str, ...]  # fully-qualified service names to transcode


@dataclass(frozen=True)
class Webhook:
    """The GitHub App webhook listener (forge-auth F6): plain HTTP/1.1 to the fleet's
    receiver, one exact `POST <path>` route, a body cap, and NO jwt_authn — GitHub sends
    no bearer; the receiver verifies each delivery's HMAC signature itself."""

    name: str
    port: int
    cluster: str
    upstream_port: int
    path: str
    max_body: int


# The receiver's own MAX_WEBHOOK_BODY; GitHub caps payloads at 25 MiB, but the
# events the fleet consumes are far smaller, so a spec asking for more is a bug.
MAX_WEBHOOK_BODY = 1 << 20


def validate_webhook(w: Webhook) -> Webhook:
    """Fail closed on a spec that would widen the unauthenticated surface: the path
    must be one exact absolute segment path (no wildcard, query or traversal) and the
    body cap positive and within MAX_WEBHOOK_BODY."""
    no_controls("the webhook path", w.path)
    segs = w.path.split("/")
    if (not w.path.startswith("/") or len(w.path) > 64 or w.path.endswith("/")
            or any(s in ("", ".", "..") for s in segs[1:])
            or any(not all(c.isalnum() or c in "-_" for c in s) for s in segs[1:])):
        raise EnvoyError(f"webhook path {w.path!r} is not an exact /segment path")
    if not 0 < w.max_body <= MAX_WEBHOOK_BODY:
        raise EnvoyError(f"webhook max_body {w.max_body} is outside 1..={MAX_WEBHOOK_BODY}")
    return w


# ---------------------------------------------------------------- the front door
#
# security-hardening 10, F4/F7/F10: one TLS origin for the portal, its RPCs and the
# observability tools, plus HyperDX on its own port. Envoy asks the agent's gate
# (`[auth.gate]`, an `ext_authz` http_service on loopback) about every request but
# sign-in and Dex, and each route's upstream sees exactly the identity headers the
# table (nix/portal/front-door-headers.json) gives it.

FRONT_DOOR_MODES = ("off", "on")
# The table's columns: the gate's routes, plus the two it never checks.
FRONT_DOOR_COLUMNS = ("rpc", "portal", "auth", "dex", "grafana", "prometheus", "hyperdx")
UNCHECKED_COLUMNS = ("auth", "dex")
HEADER_ACTIONS = ("kept", "removed", "replaced")
FRONT_DOOR_UPSTREAMS = (
    "gate", "dex", "portal", "gateway", "sessions", "fleet", "grafana", "prometheus", "hyperdx")
# The gate's own routes on the main listener; anything else under /auth/ is checked.
GATE_PATHS = ("/auth/login", "/auth/callback", "/auth/logout")
# What a denied check may send the browser (Envoy adds status and Location itself).
DENIED_CLIENT_HEADERS = ("location", "set-cookie", "cache-control", "retry-after", "content-type")
CHECK_TIMEOUT = "1s"
HSTS = "max-age=31536000"
GATE_CLUSTER = "front_gate"
HEADER_NAME = re.compile(r"[a-z0-9][a-z0-9-]{0,63}")
SERVICE_NAME = re.compile(r"agent\.v1\.[A-Z][A-Za-z0-9]{0,63}")


@dataclass(frozen=True)
class FrontListener:
    name: str
    port: int
    check: str  # the gate's check prefix for this listener (`/_check/main`)


@dataclass(frozen=True)
class FrontDoor:
    main: FrontListener
    hyperdx: FrontListener
    upstreams: Mapping[str, int]
    sessions: tuple[str, ...]  # services on the sessions process
    fleet: tuple[str, ...]  # services on the fleet process
    headers: Mapping[str, Mapping[str, str]]  # header -> column -> action

    def removed(self, column: str) -> list[str]:
        return sorted(h for h, row in self.headers.items() if row[column] == "removed")

    def replaced(self, column: str) -> list[str]:
        return sorted(h for h, row in self.headers.items() if row[column] == "replaced")


def front_listener(raw: Mapping) -> FrontListener:
    l = FrontListener(str(raw["name"]), int(raw["port"]), str(raw["check"]))
    if not re.fullmatch(r"/_check/[a-z]{1,16}", l.check):
        raise EnvoyError(f"front door check prefix {l.check!r} is not /_check/<listener>")
    if not 0 < l.port < 65536:
        raise EnvoyError(f"front door port {l.port} is out of range")
    return l


def validate_header_table(raw: Mapping) -> dict[str, dict[str, str]]:
    """F4's table, fail closed: every header a lowercase token with an action on
    every column, nothing replaced where the gate never answers, and `authorization`
    never passed through as the client sent it."""
    if tuple(raw.get("routes", ())) != FRONT_DOOR_COLUMNS:
        raise EnvoyError(f"front door header table routes must be {FRONT_DOOR_COLUMNS}")
    headers = raw.get("headers")
    if not isinstance(headers, Mapping) or not headers:
        raise EnvoyError("front door header table has no headers")
    out: dict[str, dict[str, str]] = {}
    for name, row in headers.items():
        if not isinstance(name, str) or not HEADER_NAME.fullmatch(name):
            raise EnvoyError(f"front door header {name!r} is not a lowercase header name")
        if not isinstance(row, Mapping) or set(row) != set(FRONT_DOOR_COLUMNS):
            raise EnvoyError(f"front door header {name}: needs exactly the columns {FRONT_DOOR_COLUMNS}")
        for col, action in row.items():
            if action not in HEADER_ACTIONS:
                raise EnvoyError(f"front door header {name} on {col}: {action!r} is not one of {HEADER_ACTIONS}")
            if action == "replaced" and col in UNCHECKED_COLUMNS:
                raise EnvoyError(f"front door header {name}: {col} is never checked, so nothing replaces it")
        if name == "authorization" and "kept" in row.values():
            raise EnvoyError("front door: a client's own authorization header is never passed through")
        out[name] = dict(row)
    return out


def load_front_door(raw: Mapping) -> FrontDoor:
    main, hyperdx = front_listener(raw["main"]), front_listener(raw["hyperdx"])
    if main.port == hyperdx.port or main.check == hyperdx.check:
        raise EnvoyError("the front door's two listeners need their own port and check prefix")
    up = {str(k): int(v) for k, v in raw["upstreams"].items()}
    if set(up) != set(FRONT_DOOR_UPSTREAMS):
        raise EnvoyError(f"front door upstreams must be exactly {FRONT_DOOR_UPSTREAMS}")
    for name, port in up.items():
        if not 0 < port < 65536:
            raise EnvoyError(f"front door upstream {name} port {port} is out of range")
    svc = raw["rpc_services"]
    sessions = tuple(str(x) for x in svc["sessions"])
    fleet = tuple(str(x) for x in svc["fleet"])
    for name in sessions + fleet:
        if not SERVICE_NAME.fullmatch(name):
            raise EnvoyError(f"front door rpc service {name!r} is not an agent.v1 service name")
    if len(set(sessions + fleet)) != len(sessions + fleet):
        raise EnvoyError("a front door rpc service is routed to two processes")
    return FrontDoor(main, hyperdx, up, sessions, fleet, validate_header_table(raw["headers"]))


# The transcoder `services` list is derived from the descriptor (nix/rest-descriptor.nix);
# a value far above the ~40-service surface means a malformed services file, so cap it.
MAX_SERVICES = 512


def load_services(path: str) -> tuple[str, ...]:
    """The transcoder's service list, one FQN per line. Every name must be a
    fully-qualified `agent.v1.*` service — the descriptor and this list come from the
    same build, so anything else (a google.api service, an unversioned name, an empty
    file) is a build-shape bug the renderer must fail closed on, not paper over."""
    try:
        raw = Path(path).read_text()
    except OSError as e:
        raise EnvoyError(f"cannot read the transcoder services file: {e.strerror}") from None
    names = [n.strip() for n in raw.splitlines() if n.strip()]
    if not names:
        raise EnvoyError("the transcoder services file is empty")
    if len(names) > MAX_SERVICES:
        raise EnvoyError(f"the transcoder services file lists more than {MAX_SERVICES} services")
    for n in names:
        no_controls("a transcoder service name", n)
        # Fully-qualified `agent.v1.<Name>` — the same shape the rest-descriptor check
        # asserts. A name outside this shape means the extraction leaked something.
        parts = n.split(".")
        if len(parts) != 3 or parts[0] != "agent" or parts[1] != "v1" or not parts[2].isidentifier():
            raise EnvoyError(f"transcoder service {n!r} is not a fully-qualified agent.v1 name")
    # Deterministic + de-duplicated: the config must be byte-stable across renders.
    return tuple(sorted(set(names)))


@dataclass(frozen=True)
class Spec:
    """What nix knows: the listeners, the collector, the agent's protos."""

    listeners: tuple[Listener, ...]
    otel_port: int
    gateway_port: int
    rest: Rest | None = None
    webhook: Webhook | None = None
    front_door: FrontDoor | None = None

    @staticmethod
    def load(raw: Mapping) -> "Spec":
        try:
            listeners = tuple(
                Listener(str(l["name"]), int(l["port"]), str(l["cluster"]), int(l["upstream_port"]))
                for l in raw["listeners"]
            )
            rest = None
            r = raw.get("rest")
            if r is not None:
                rest = Rest(
                    str(r["name"]), int(r["port"]), str(r["cluster"]),
                    int(r["upstream_port"]), str(r["descriptor"]),
                    load_services(str(r["services_file"])),
                )
            webhook = None
            w = raw.get("webhook")
            if w is not None:
                webhook = validate_webhook(Webhook(
                    str(w["name"]), int(w["port"]), str(w["cluster"]),
                    int(w["upstream_port"]), str(w["path"]), int(w["max_body"])))
            fd = raw.get("front_door")
            front_door = load_front_door(fd) if fd is not None else None
            return Spec(listeners, int(raw["otel_port"]), int(raw["gateway_port"]), rest, webhook,
                        front_door)
        except (KeyError, TypeError, ValueError) as e:
            raise EnvoyError(f"bad spec: {e}") from e


@dataclass(frozen=True)
class Tls:
    """PEM file paths as the rendered config should name them."""

    cert: str
    key: str


@dataclass(frozen=True)
class Upstream:
    ca: str
    sni: str
    client: Tls | None = None


@dataclass(frozen=True)
class Jwt:
    issuer: str
    audiences: tuple[str, ...]
    jwks_inline: str | None = None  # a JWK Set document
    jwks_url: str | None = None


@dataclass
class Knobs:
    host: str = "127.0.0.1"
    origins: tuple[str, ...] = DEFAULT_ORIGINS
    auth: str = "auto"
    jwks: str = ""
    jwks_from: str = ""
    jwks_wait: int = DEFAULT_JWKS_WAIT
    issuer: str = ""
    audiences: tuple[str, ...] = ()
    tls_cert: str = ""
    tls_key: str = ""
    upstream_ca: str = ""
    upstream_cert: str = ""
    upstream_key: str = ""
    upstream_sni: str = "localhost"
    otlp_authorization: str = ""
    front_door: str = "off"
    front_host: str = "127.0.0.1"
    front_cert: str = ""
    front_key: str = ""
    front_origins: tuple[str, ...] = ()
    notes: list[str] = field(default_factory=list)


# ---------------------------------------------------------------- validation


def no_controls(what: str, value: str) -> str:
    if any(ord(c) < 0x20 or ord(c) == 0x7F for c in value):
        raise EnvoyError(f"{what} contains a control character")
    return value


def validate_host(value: str) -> str:
    """An IP literal: Envoy binds a socket_address, and a name would need a resolver."""
    try:
        return str(ipaddress.ip_address(value.strip()))
    except ValueError:
        raise EnvoyError(f"PORTAL_GRPC_WEB_HOST {value!r} is not an IP address") from None


def is_loopback_host(host: str) -> bool:
    try:
        return ipaddress.ip_address(host).is_loopback
    except ValueError:
        return False


def validate_origin(value: str) -> str:
    """One exact browser origin, normalised to scheme://host[:port]."""
    raw = no_controls("PORTAL_WEB_ORIGIN", value.strip())
    if not raw or "*" in raw or raw.lower() == "null":
        raise EnvoyError(f"origin {value!r} must be one exact scheme://host[:port]; no wildcards")
    try:
        u = urlsplit(raw)
        port = u.port
    except ValueError:
        raise EnvoyError(f"origin {value!r} is not a valid URL") from None
    if u.scheme not in ("http", "https"):
        raise EnvoyError(f"origin {value!r} must be http or https")
    if not u.hostname or u.username is not None or u.password is not None:
        raise EnvoyError(f"origin {value!r} needs a host and no credentials")
    if u.path not in ("", "/") or u.query or u.fragment:
        raise EnvoyError(f"origin {value!r} must not carry a path, query or fragment")
    host = u.hostname
    if ":" in host:  # IPv6 literal
        host = f"[{host}]"
    return f"{u.scheme}://{host}" + (f":{port}" if port is not None else "")


def validate_origins(value: str) -> tuple[str, ...]:
    parts = [p for p in value.split(",") if p.strip()]
    if not parts:
        raise EnvoyError("PORTAL_WEB_ORIGIN is empty")
    if len(parts) > MAX_ORIGINS:
        raise EnvoyError(f"PORTAL_WEB_ORIGIN lists more than {MAX_ORIGINS} origins")
    out: list[str] = []
    for p in parts:
        o = validate_origin(p)
        if o not in out:
            out.append(o)
    return tuple(out)


def validate_jwks_url(value: str) -> str:
    """https, or http only to a loopback IP (the agent's own `check_fetch_url` rule)."""
    raw = no_controls("PORTAL_JWT_JWKS", value.strip())
    try:
        u = urlsplit(raw)
        u.port
    except ValueError:
        raise EnvoyError(f"PORTAL_JWT_JWKS {value!r} is not a valid URL") from None
    if not u.hostname or u.username is not None or u.password is not None:
        raise EnvoyError("PORTAL_JWT_JWKS needs a host and no credentials")
    if u.scheme == "https":
        return raw
    if u.scheme == "http" and is_loopback_host(u.hostname):
        return raw
    raise EnvoyError("PORTAL_JWT_JWKS must use https (plain http only to a loopback IP)")


def validate_jwks(doc: str) -> str:
    """A JWK Set with at least one key; returned compact."""
    if len(doc.encode()) > MAX_JWKS_BYTES:
        raise EnvoyError(f"JWKS is larger than {MAX_JWKS_BYTES} bytes")
    try:
        parsed = json.loads(doc)
    except ValueError:
        raise EnvoyError("JWKS is not JSON") from None
    keys = parsed.get("keys") if isinstance(parsed, dict) else None
    if not isinstance(keys, list) or not keys:
        raise EnvoyError("JWKS has no keys")
    for k in keys:
        if not isinstance(k, dict) or not isinstance(k.get("kty"), str):
            raise EnvoyError("JWKS key without a `kty`")
        if any(p in k for p in ("d", "p", "q", "dp", "dq", "qi", "k")):
            raise EnvoyError("JWKS carries private key material; refusing to render it")
    return json.dumps(parsed, separators=(",", ":"), sort_keys=True)


def validate_hostport(value: str) -> str:
    raw = no_controls("PORTAL_JWKS_FROM", value.strip())
    host, sep, port = raw.rpartition(":")
    if not sep or not host or not port.isdigit() or not 0 < int(port) < 65536:
        raise EnvoyError(f"PORTAL_JWKS_FROM {value!r} must be host:port")
    if host.startswith("-") or any(c in host for c in " /@"):
        raise EnvoyError(f"PORTAL_JWKS_FROM {value!r} has an invalid host")
    return raw


def validate_sni(value: str) -> str:
    raw = no_controls("PORTAL_UPSTREAM_SNI", value.strip())
    if not raw or len(raw) > 253 or any(c in raw for c in " /:@*"):
        raise EnvoyError(f"PORTAL_UPSTREAM_SNI {value!r} is not a DNS name")
    return raw


def knobs_from_env(env: Mapping[str, str], spec: Spec) -> Knobs:
    g = lambda k: env.get(k, "").strip()  # noqa: E731
    k = Knobs()
    if g("PORTAL_GRPC_WEB_HOST"):
        k.host = validate_host(g("PORTAL_GRPC_WEB_HOST"))
    if g("PORTAL_WEB_ORIGIN"):
        k.origins = validate_origins(g("PORTAL_WEB_ORIGIN"))
    k.auth = g("PORTAL_AUTH") or "auto"
    if k.auth not in AUTH_MODES:
        raise EnvoyError(f"PORTAL_AUTH must be one of {'|'.join(AUTH_MODES)}, not {k.auth!r}")
    k.jwks = g("PORTAL_JWT_JWKS")
    k.jwks_from = validate_hostport(g("PORTAL_JWKS_FROM") or f"127.0.0.1:{spec.gateway_port}")
    wait = g("PORTAL_JWKS_WAIT") or str(DEFAULT_JWKS_WAIT)
    if not wait.isdigit() or int(wait) > MAX_JWKS_WAIT:
        raise EnvoyError(f"PORTAL_JWKS_WAIT must be 0..{MAX_JWKS_WAIT} seconds")
    k.jwks_wait = int(wait)
    k.issuer = no_controls("PORTAL_JWT_ISSUER", g("PORTAL_JWT_ISSUER"))
    k.audiences = tuple(
        no_controls("PORTAL_JWT_AUDIENCE", a.strip())
        for a in g("PORTAL_JWT_AUDIENCE").split(",")
        if a.strip()
    )
    k.tls_cert, k.tls_key = g("PORTAL_TLS_CERT"), g("PORTAL_TLS_KEY")
    if bool(k.tls_cert) != bool(k.tls_key):
        raise EnvoyError("PORTAL_TLS_CERT and PORTAL_TLS_KEY go together")
    k.upstream_ca = g("PORTAL_UPSTREAM_CA")
    k.upstream_cert, k.upstream_key = g("PORTAL_UPSTREAM_CERT"), g("PORTAL_UPSTREAM_KEY")
    if bool(k.upstream_cert) != bool(k.upstream_key):
        raise EnvoyError("PORTAL_UPSTREAM_CERT and PORTAL_UPSTREAM_KEY go together")
    if k.upstream_cert and not k.upstream_ca:
        raise EnvoyError("PORTAL_UPSTREAM_CERT needs PORTAL_UPSTREAM_CA (mTLS verifies the agent too)")
    k.upstream_sni = validate_sni(g("PORTAL_UPSTREAM_SNI") or "localhost")
    k.otlp_authorization = no_controls(
        "the OTLP ingestion key",
        env.get("PORTAL_OTLP_AUTHORIZATION") or env.get("CLICKSTACK_INGESTION_API_KEY") or "",
    )
    front_door_knobs(env, spec, k)
    for path in (k.tls_cert, k.tls_key, k.upstream_ca, k.upstream_cert, k.upstream_key,
                 k.front_cert, k.front_key):
        if path and not os.path.isabs(path):
            raise EnvoyError(f"{path!r} must be an absolute path")
    if not is_loopback_host(k.host):
        k.notes.append(
            f"listening on {k.host}: reachable beyond this host; CORS allows only "
            + ", ".join(k.origins)
        )
    return k


def front_door_knobs(env: Mapping[str, str], spec: Spec, k: Knobs) -> None:
    g = lambda n: env.get(n, "").strip()  # noqa: E731
    k.front_door = g("PORTAL_FRONT_DOOR") or "off"
    if k.front_door not in FRONT_DOOR_MODES:
        raise EnvoyError(f"PORTAL_FRONT_DOOR must be one of {'|'.join(FRONT_DOOR_MODES)}, not {k.front_door!r}")
    extra = [n for n in ("PORTAL_FRONT_DOOR_CERT", "PORTAL_FRONT_DOOR_KEY", "PORTAL_FRONT_DOOR_HOST",
                         "PORTAL_FRONT_DOOR_ORIGINS") if g(n)]
    if k.front_door == "off":
        if extra:
            raise EnvoyError(f"{', '.join(extra)} set but PORTAL_FRONT_DOOR is not on")
        return
    if spec.front_door is None:
        raise EnvoyError("PORTAL_FRONT_DOOR=on but the spec has no front_door")
    k.front_cert, k.front_key = g("PORTAL_FRONT_DOOR_CERT"), g("PORTAL_FRONT_DOOR_KEY")
    if not (k.front_cert and k.front_key):
        raise EnvoyError("the front door is HTTPS only: set PORTAL_FRONT_DOOR_CERT and PORTAL_FRONT_DOOR_KEY")
    if g("PORTAL_FRONT_DOOR_HOST"):
        k.front_host = validate_host(g("PORTAL_FRONT_DOOR_HOST"))
    if g("PORTAL_FRONT_DOOR_ORIGINS"):
        k.front_origins = validate_origins(g("PORTAL_FRONT_DOOR_ORIGINS"))
        if any(o.startswith("http://") for o in k.front_origins):
            raise EnvoyError("PORTAL_FRONT_DOOR_ORIGINS: the front door is https, so are its other origins")


# ---------------------------------------------------------------- the JWKS


Runner = Callable[[Sequence[str]], "subprocess.CompletedProcess[str]"]


def run(argv: Sequence[str]) -> "subprocess.CompletedProcess[str]":
    return subprocess.run(list(argv), capture_output=True, text=True, timeout=30)


def fetch_jwks(
    grpcurl: str,
    proto_dir: str,
    target: str,
    wait: int,
    runner: Runner = run,
    sleep: Callable[[float], None] = time.sleep,
) -> str | None:
    """AuthService.Jwks from the agent. None ⇒ it serves no agent tokens.

    An unreachable agent is retried until `wait` seconds pass, then is an error: the
    edge cannot tell "no tokens" from "not up yet", and guessing would fail open.
    """
    argv = [
        grpcurl, "-plaintext", "-max-time", "5",
        "-import-path", proto_dir, "-proto", "agent/v1/auth.proto",
        "-d", "{}", target, "agent.v1.AuthService/Jwks",
    ]
    deadline = time.monotonic() + wait
    while True:
        cp = runner(argv)
        if cp.returncode == 0:
            try:
                doc = json.loads(cp.stdout or "{}").get("jwksJson", "")
            except ValueError:
                raise EnvoyError("AuthService.Jwks answered with something that is not JSON") from None
            if not doc:
                return None
            try:
                keys = json.loads(doc).get("keys")
            except (ValueError, AttributeError):
                keys = None
            return doc if keys else None
        err = cp.stderr or ""
        if "Unimplemented" in err:
            return None
        if time.monotonic() >= deadline:
            raise EnvoyError(
                f"could not reach the agent at {target} for its JWKS ({err.strip()[:200]}); "
                "start the gateway first, set PORTAL_JWT_JWKS, or PORTAL_AUTH=off"
            )
        sleep(1)


def resolve_jwt(k: Knobs, fetch: Callable[[], str | None], read: Callable[[str], str]) -> Jwt | None:
    """The jwt_authn provider to render, or None for no edge check."""
    if k.auth == "off":
        k.notes.append("PORTAL_AUTH=off: no jwt_authn at the edge; the agent still verifies")
        return None
    if k.jwks:
        if "://" in k.jwks:
            return Jwt(k.issuer, k.audiences, jwks_url=validate_jwks_url(k.jwks))
        if not os.path.isabs(k.jwks):
            raise EnvoyError("PORTAL_JWT_JWKS must be an absolute path or a URL")
        try:
            doc = read(k.jwks)
        except OSError as e:
            raise EnvoyError(f"cannot read PORTAL_JWT_JWKS: {e.strerror}") from None
        return Jwt(k.issuer, k.audiences, jwks_inline=validate_jwks(doc))
    doc = fetch()
    if doc is None:
        if k.auth == "on":
            raise EnvoyError("PORTAL_AUTH=on but the agent serves no agent tokens (no JWKS)")
        k.notes.append("the agent serves no agent tokens: no jwt_authn at the edge")
        return None
    return Jwt(k.issuer, k.audiences, jwks_inline=validate_jwks(doc))


# ---------------------------------------------------------------- rendering


def any_type(name: str) -> str:
    return f"type.googleapis.com/{name}"


def cluster(name: str, host: str, port: int, h2: bool = True, tls: dict | None = None) -> dict:
    c: dict = {
        "name": name,
        "connect_timeout": "0.25s",
        "type": "LOGICAL_DNS",
        "lb_policy": "ROUND_ROBIN",
        "load_assignment": {
            "cluster_name": name,
            "endpoints": [{"lb_endpoints": [{"endpoint": {"address": {
                "socket_address": {"address": host, "port_value": port}}}}]}],
        },
    }
    if h2:
        c["typed_extension_protocol_options"] = {
            "envoy.extensions.upstreams.http.v3.HttpProtocolOptions": {
                "@type": any_type("envoy.extensions.upstreams.http.v3.HttpProtocolOptions"),
                "explicit_http_config": {"http2_protocol_options": {}},
            }
        }
    if tls is not None:
        c["transport_socket"] = {
            "name": "envoy.transport_sockets.tls",
            "typed_config": {
                "@type": any_type("envoy.extensions.transport_sockets.tls.v3.UpstreamTlsContext"),
                **tls,
            },
        }
    return c


def upstream_tls(up: Upstream) -> dict:
    common: dict = {
        "validation_context": {
            "trusted_ca": {"filename": up.ca},
            "match_typed_subject_alt_names": [
                {"san_type": "DNS", "matcher": {"exact": up.sni}}
            ],
        }
    }
    if up.client:
        common["tls_certificates"] = [{
            "certificate_chain": {"filename": up.client.cert},
            "private_key": {"filename": up.client.key},
        }]
    return {"sni": up.sni, "common_tls_context": common}


def otlp_grpc_service(auth: str) -> dict:
    return {
        "envoy_grpc": {"cluster_name": "otel_collector"},
        "initial_metadata": [{"key": "authorization", "value": auth}],
    }


def access_log(auth: str) -> list:
    attrs = [
        ("duration_ms", "%DURATION%"),
        ("response_duration_ms", "%RESPONSE_DURATION%"),
        ("request_duration_ms", "%REQUEST_DURATION%"),
        ("grpc_status", "%GRPC_STATUS%"),
        ("response_code", "%RESPONSE_CODE%"),
        ("response_flags", "%RESPONSE_FLAGS%"),
        ("upstream_host", "%UPSTREAM_HOST%"),
        ("request_id", "%REQ(X-REQUEST-ID)%"),
    ]
    return [{
        "name": "envoy.access_loggers.open_telemetry",
        "typed_config": {
            "@type": any_type("envoy.extensions.access_loggers.open_telemetry.v3.OpenTelemetryAccessLogConfig"),
            "common_config": {
                "log_name": "envoy-portal-bridge",
                "transport_api_version": "V3",
                "grpc_service": otlp_grpc_service(auth),
            },
            "resource_attributes": {"values": [
                {"key": "service.name", "value": {"string_value": "envoy-portal-bridge"}}]},
            "body": {"string_value": "%REQ(:PATH)%"},
            "attributes": {"values": [
                {"key": k, "value": {"string_value": v}} for k, v in attrs]},
        },
    }]


def tracing(auth: str) -> dict:
    svc = otlp_grpc_service(auth)
    svc["timeout"] = "0.250s"
    return {
        "random_sampling": {"value": 100},
        "provider": {
            "name": "envoy.tracers.opentelemetry",
            "typed_config": {
                "@type": any_type("envoy.config.trace.v3.OpenTelemetryConfig"),
                "service_name": "envoy-portal-bridge",
                "grpc_service": svc,
            },
        },
    }


def cors_policy(origins: Sequence[str]) -> dict:
    return {
        "@type": any_type("envoy.extensions.filters.http.cors.v3.CorsPolicy"),
        "allow_origin_string_match": [{"exact": o} for o in origins],
        "allow_methods": "GET, PUT, DELETE, POST, OPTIONS",
        "allow_headers": ALLOW_HEADERS,
        "max_age": "1728000",
        "expose_headers": EXPOSE_HEADERS,
    }


def jwt_filter(jwt: Jwt) -> dict:
    provider: dict = {"forward": True}
    if jwt.issuer:
        provider["issuer"] = jwt.issuer
    if jwt.audiences:
        provider["audiences"] = list(jwt.audiences)
    if jwt.jwks_inline is not None:
        provider["local_jwks"] = {"inline_string": jwt.jwks_inline}
    else:
        provider["remote_jwks"] = {
            "http_uri": {"uri": jwt.jwks_url, "cluster": JWKS_CLUSTER, "timeout": "5s"},
            "cache_duration": "300s",
            "async_fetch": {},
        }
    rules: list = [{"match": {"prefix": p}} for p in UNAUTHENTICATED_PREFIXES]
    rules.append({"match": {"prefix": "/"}, "requires": {"provider_name": PROVIDER}})
    return {
        "name": "envoy.filters.http.jwt_authn",
        "typed_config": {
            "@type": any_type("envoy.extensions.filters.http.jwt_authn.v3.JwtAuthentication"),
            "providers": {PROVIDER: provider},
            "rules": rules,
            "bypass_cors_preflight": True,
        },
    }


def http_filters(jwt: Jwt | None) -> list:
    def f(name: str, typ: str) -> dict:
        return {"name": name, "typed_config": {"@type": any_type(typ)}}

    # CORS answers the preflight before jwt_authn sees it, and decorates its 401.
    out = [
        f("envoy.filters.http.grpc_web", "envoy.extensions.filters.http.grpc_web.v3.GrpcWeb"),
        f("envoy.filters.http.cors", "envoy.extensions.filters.http.cors.v3.Cors"),
    ]
    if jwt is not None:
        out.append(jwt_filter(jwt))
    out.append(f("envoy.filters.http.router", "envoy.extensions.filters.http.router.v3.Router"))
    return out


def listener(l: Listener, k: Knobs, jwt: Jwt | None, tls: Tls | None) -> dict:
    hcm = {
        "@type": any_type(
            "envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager"),
        "stat_prefix": l.name,
        "codec_type": "AUTO",
        "access_log": access_log(k.otlp_authorization),
        "tracing": tracing(k.otlp_authorization),
        "route_config": {
            "name": f"{l.name}_route",
            "virtual_hosts": [{
                "name": l.cluster,
                "domains": ["*"],
                "typed_per_filter_config": {"envoy.filters.http.cors": cors_policy(k.origins)},
                "routes": [{"match": {"prefix": "/"}, "route": {"cluster": l.cluster, "timeout": "0s"}}],
            }],
        },
        "http_filters": http_filters(jwt),
    }
    chain: dict = {"filters": [{
        "name": "envoy.filters.network.http_connection_manager", "typed_config": hcm}]}
    if tls is not None:
        chain["transport_socket"] = {
            "name": "envoy.transport_sockets.tls",
            "typed_config": {
                "@type": any_type("envoy.extensions.transport_sockets.tls.v3.DownstreamTlsContext"),
                "common_tls_context": {
                    "alpn_protocols": ["h2", "http/1.1"],
                    "tls_certificates": [{
                        "certificate_chain": {"filename": tls.cert},
                        "private_key": {"filename": tls.key},
                    }],
                },
            },
        }
    return {
        "name": l.name,
        "address": {"socket_address": {"address": k.host, "port_value": l.port}},
        "filter_chains": [chain],
    }


def transcoder_filter(descriptor_path: str, services: Sequence[str]) -> dict:
    """`grpc_json_transcoder`: HTTP+JSON ⇄ gRPC per the descriptor's `(google.api.http)`
    routes. `auto_mapping: false` because every RPC is explicitly annotated;
    `match_incoming_request_route: true` so an unmapped path falls through (404) instead
    of being force-mapped; validation rejects unknown methods/query params (the request
    body is attacker-controlled — fail closed)."""
    return {
        "name": "envoy.filters.http.grpc_json_transcoder",
        "typed_config": {
            "@type": any_type(
                "envoy.extensions.filters.http.grpc_json_transcoder.v3.GrpcJsonTranscoder"),
            "proto_descriptor": descriptor_path,
            "services": list(services),
            "auto_mapping": False,
            "match_incoming_request_route": True,
            "convert_grpc_status": True,
            "request_validation_options": {
                "reject_unknown_method": True,
                "reject_unknown_query_parameters": True,
            },
            # `add_whitespace: false` — emit compact JSON. Pretty-printing nearly DOUBLES a
            # config read on the wire (measured: GET /v1/config/values 35,078 B pretty →
            # 17,878 B compact, 49% saved) for no machine-client benefit, and compact output
            # avoids formatting attacker-influenced field values into the response body.
            # `always_print_primitive_fields` stays true — that is an API-shape contract
            # (zero-valued fields remain present), not a formatting toggle.
            "print_options": {"add_whitespace": False, "always_print_primitive_fields": True},
        },
    }


def rest_listener(rest: Rest, k: Knobs, descriptor_path: str, jwt: Jwt | None, tls: Tls | None) -> dict:
    """The REST/JSON transcoder listener. Filter order `cors ▶ grpc_json_transcoder ▶
    jwt_authn ▶ router` (the transcoder must precede the router; jwt_authn sits after
    it — see below). No `grpc_web`: this is plain HTTP+JSON, not grpc-web framing.

    Edge `jwt_authn` runs AFTER the transcoder because `grpc_json_transcoder` rewrites
    `:path` to the gRPC method path while decoding headers, so the SAME
    `UNAUTHENTICATED_PREFIXES` (gRPC paths — `/agent.v1.AuthService/` etc.) that guard
    the grpc-web listeners apply here unchanged: one source of truth, no second
    REST-path list to drift. This is **fail-closed** — a REST path that is unmapped, or
    that reaches jwt_authn un-rewritten, never matches a gRPC exempt prefix, so it falls
    to the catch-all `/` requires-token rule (401); it can never fall open. When auth is
    off (`jwt is None`) there is no edge check and the agent's AuthLayer still verifies
    every transcoded call. Bind is FIXED 127.0.0.1 — it does not follow
    PORTAL_GRPC_WEB_HOST, so a LAN bind of the grpc-web listeners never silently exposes
    REST, and edge auth is defense-in-depth on top of that loopback pin."""
    filters: list = [
        {"name": "envoy.filters.http.cors",
         "typed_config": {"@type": any_type("envoy.extensions.filters.http.cors.v3.Cors")}},
        transcoder_filter(descriptor_path, rest.services),
    ]
    if jwt is not None:
        filters.append(jwt_filter(jwt))
    filters.append(
        {"name": "envoy.filters.http.router",
         "typed_config": {"@type": any_type("envoy.extensions.filters.http.router.v3.Router")}})
    hcm = {
        "@type": any_type(
            "envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager"),
        "stat_prefix": rest.name,
        "codec_type": "AUTO",
        "access_log": access_log(k.otlp_authorization),
        "tracing": tracing(k.otlp_authorization),
        "route_config": {
            "name": f"{rest.name}_route",
            "virtual_hosts": [{
                "name": rest.cluster,
                "domains": ["*"],
                "typed_per_filter_config": {"envoy.filters.http.cors": cors_policy(k.origins)},
                "routes": [{"match": {"prefix": "/"},
                            "route": {"cluster": rest.cluster, "timeout": "0s"}}],
            }],
        },
        "http_filters": filters,
    }
    chain: dict = {"filters": [{
        "name": "envoy.filters.network.http_connection_manager", "typed_config": hcm}]}
    if tls is not None:
        chain["transport_socket"] = {
            "name": "envoy.transport_sockets.tls",
            "typed_config": {
                "@type": any_type("envoy.extensions.transport_sockets.tls.v3.DownstreamTlsContext"),
                "common_tls_context": {
                    "alpn_protocols": ["h2", "http/1.1"],
                    "tls_certificates": [{
                        "certificate_chain": {"filename": tls.cert},
                        "private_key": {"filename": tls.key},
                    }],
                },
            },
        }
    return {
        "name": rest.name,
        "address": {"socket_address": {"address": "127.0.0.1", "port_value": rest.port}},
        "filter_chains": [chain],
    }


def webhook_listener(w: Webhook, k: Knobs, tls: Tls | None) -> dict:
    """The GitHub App webhook listener. Filter chain `buffer ▶ router`: no `grpc_web`
    (plain HTTP), no CORS (no browser calls it), and **no jwt_authn** — the receiver's
    HMAC check is the authentication, so nothing here may forward a path the receiver
    does not verify. Hence ONE route, exact `path` + `:method POST`, and a catch-all
    direct 404: the agent's other surfaces are never reachable through this listener.
    `buffer` enforces the body cap at the edge (413), and the header/request timeouts
    absorb slow clients the receiver's tiny_http would otherwise hold a thread for.
    Binds the host knob (GitHub must reach it) and terminates the same downstream TLS;
    the upstream hop is plain HTTP to the loopback receiver."""
    hcm = {
        "@type": any_type(
            "envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager"),
        "stat_prefix": w.name,
        "codec_type": "AUTO",
        "request_headers_timeout": "5s",
        "request_timeout": "15s",
        "max_request_headers_kb": 32,
        "access_log": access_log(k.otlp_authorization),
        "tracing": tracing(k.otlp_authorization),
        "route_config": {
            "name": f"{w.name}_route",
            "virtual_hosts": [{
                "name": w.cluster,
                "domains": ["*"],
                "routes": [
                    {"match": {"path": w.path, "headers": [
                        {"name": ":method", "string_match": {"exact": "POST"}}]},
                     # GitHub abandons a delivery after 10s; the receiver answers 202 at once.
                     "route": {"cluster": w.cluster, "timeout": "10s"}},
                    {"match": {"prefix": "/"}, "direct_response": {"status": 404}},
                ],
            }],
        },
        "http_filters": [
            {"name": "envoy.filters.http.buffer", "typed_config": {
                "@type": any_type("envoy.extensions.filters.http.buffer.v3.Buffer"),
                "max_request_bytes": w.max_body}},
            {"name": "envoy.filters.http.router",
             "typed_config": {"@type": any_type("envoy.extensions.filters.http.router.v3.Router")}},
        ],
    }
    chain: dict = {"filters": [{
        "name": "envoy.filters.network.http_connection_manager", "typed_config": hcm}]}
    if tls is not None:
        chain["transport_socket"] = {
            "name": "envoy.transport_sockets.tls",
            "typed_config": {
                "@type": any_type("envoy.extensions.transport_sockets.tls.v3.DownstreamTlsContext"),
                "common_tls_context": {
                    "alpn_protocols": ["h2", "http/1.1"],
                    "tls_certificates": [{
                        "certificate_chain": {"filename": tls.cert},
                        "private_key": {"filename": tls.key},
                    }],
                },
            },
        }
    return {
        "name": w.name,
        "address": {"socket_address": {"address": k.host, "port_value": w.port}},
        "filter_chains": [chain],
    }


def downstream_tls(tls: Tls) -> dict:
    return {
        "name": "envoy.transport_sockets.tls",
        "typed_config": {
            "@type": any_type("envoy.extensions.transport_sockets.tls.v3.DownstreamTlsContext"),
            "common_tls_context": {
                "alpn_protocols": ["h2", "http/1.1"],
                "tls_certificates": [{
                    "certificate_chain": {"filename": tls.cert},
                    "private_key": {"filename": tls.key},
                }],
            },
        },
    }


def ext_authz_filter(fd: FrontDoor, l: FrontListener, upstream: Sequence[str]) -> dict:
    """Envoy's check with the gate, fail closed: an error or a 1 s timeout denies.
    The gate sees only the cookie and the bearer (plus method, path and host); a 200
    may set exactly `upstream`, overwriting what the client sent."""
    response: dict = {
        "allowed_client_headers": {"patterns": [{"exact": h} for h in DENIED_CLIENT_HEADERS]},
    }
    if upstream:
        response["allowed_upstream_headers"] = {"patterns": [{"exact": h} for h in upstream]}
    return {
        "name": "envoy.filters.http.ext_authz",
        "typed_config": {
            "@type": any_type("envoy.extensions.filters.http.ext_authz.v3.ExtAuthz"),
            "http_service": {
                "server_uri": {
                    "uri": f"http://127.0.0.1:{fd.upstreams['gate']}",
                    "cluster": GATE_CLUSTER,
                    "timeout": CHECK_TIMEOUT,
                },
                "path_prefix": l.check,
                "authorization_response": response,
            },
            "allowed_headers": {"patterns": [{"exact": "cookie"}, {"exact": "authorization"}]},
            "failure_mode_allow": False,
        },
    }


def unchecked() -> dict:
    return {"envoy.filters.http.ext_authz": {
        "@type": any_type("envoy.extensions.filters.http.ext_authz.v3.ExtAuthzPerRoute"),
        "disabled": True,
    }}


def front_route(match: dict, cluster_name: str, removed: Sequence[str], timeout: str = "60s",
                check: bool = True) -> dict:
    r: dict = {"match": match, "route": {"cluster": cluster_name, "timeout": timeout}}
    if removed:
        r["request_headers_to_remove"] = list(removed)
    if not check:
        r["typed_per_filter_config"] = unchecked()
    return r


def front_routes(fd: FrontDoor) -> list[dict]:
    """The main listener's routes, most specific first. `path_separated_prefix`
    matches `/dex` and `/dex/…` but never `/dexfoo`; the gate's own routes are exact
    paths, so the check prefixes can't be reached through them."""
    out = [front_route({"path": p}, GATE_CLUSTER, fd.removed("auth"), "15s", check=False)
           for p in GATE_PATHS]
    out.append(front_route({"path_separated_prefix": "/dex"}, "front_dex", fd.removed("dex"),
                           "15s", check=False))
    rpc = fd.removed("rpc")
    for cluster_name, services in (("front_rpc_sessions", fd.sessions), ("front_rpc_fleet", fd.fleet)):
        out += [front_route({"prefix": f"/{s}/"}, cluster_name, rpc, "0s") for s in services]
    out.append(front_route({"prefix": "/agent.v1."}, "front_rpc_gateway", rpc, "0s"))
    out.append(front_route({"path_separated_prefix": "/grafana"}, "front_grafana", fd.removed("grafana")))
    out.append(front_route({"path_separated_prefix": "/prometheus"}, "front_prometheus",
                           fd.removed("prometheus")))
    out.append(front_route({"prefix": "/"}, "front_portal", fd.removed("portal")))
    return out


def front_listener_config(name: str, port: int, k: Knobs, tls: Tls, routes: list[dict],
                          filters: list[dict], cors: Sequence[str]) -> dict:
    vhost: dict = {
        "name": name,
        "domains": ["*"],
        "routes": routes,
        "response_headers_to_add": [{
            "header": {"key": "strict-transport-security", "value": HSTS},
            "append_action": "OVERWRITE_IF_EXISTS_OR_ADD",
        }],
    }
    if cors:
        vhost["typed_per_filter_config"] = {"envoy.filters.http.cors": cors_policy(cors)}
    hcm = {
        "@type": any_type(
            "envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager"),
        "stat_prefix": name,
        "codec_type": "AUTO",
        "use_remote_address": True,
        "normalize_path": True,
        "merge_slashes": True,
        "path_with_escaped_slashes_action": "REJECT_REQUEST",
        "request_headers_timeout": "10s",
        "access_log": access_log(k.otlp_authorization),
        "tracing": tracing(k.otlp_authorization),
        "route_config": {"name": f"{name}_route", "virtual_hosts": [vhost]},
        "http_filters": filters,
    }
    return {
        "name": name,
        "address": {"socket_address": {"address": k.front_host, "port_value": port}},
        "filter_chains": [{
            "filters": [{"name": "envoy.filters.network.http_connection_manager", "typed_config": hcm}],
            "transport_socket": downstream_tls(tls),
        }],
    }


def front_door(fd: FrontDoor, k: Knobs, tls: Tls, up: Upstream | None) -> tuple[list[dict], list[dict]]:
    """The front door's listeners and clusters. No CORS unless an origin is listed
    (the portal and its RPCs share :8443), and then never credentialed (F10)."""
    def f(name: str, typ: str) -> dict:
        return {"name": name, "typed_config": {"@type": any_type(typ)}}

    cors = [f("envoy.filters.http.cors", "envoy.extensions.filters.http.cors.v3.Cors")] if k.front_origins else []
    router = f("envoy.filters.http.router", "envoy.extensions.filters.http.router.v3.Router")
    main_upstream = sorted({h for c in FRONT_DOOR_COLUMNS if c not in UNCHECKED_COLUMNS + ("hyperdx",)
                            for h in fd.replaced(c)})
    main = front_listener_config(
        fd.main.name, fd.main.port, k, tls, front_routes(fd),
        cors + [ext_authz_filter(fd, fd.main, main_upstream),
                f("envoy.filters.http.grpc_web", "envoy.extensions.filters.http.grpc_web.v3.GrpcWeb"),
                router],
        k.front_origins)
    hyperdx = front_listener_config(
        fd.hyperdx.name, fd.hyperdx.port, k, tls,
        [front_route({"prefix": "/"}, "front_hyperdx", fd.removed("hyperdx"))],
        [ext_authz_filter(fd, fd.hyperdx, fd.replaced("hyperdx")), router], ())
    u = fd.upstreams
    rpc_tls = upstream_tls(up) if up else None
    clusters = [
        cluster(GATE_CLUSTER, "127.0.0.1", u["gate"], h2=False),
        cluster("front_dex", "127.0.0.1", u["dex"], h2=False),
        cluster("front_portal", "127.0.0.1", u["portal"], h2=False),
        cluster("front_grafana", "127.0.0.1", u["grafana"], h2=False),
        cluster("front_prometheus", "127.0.0.1", u["prometheus"], h2=False),
        cluster("front_hyperdx", "127.0.0.1", u["hyperdx"], h2=False),
        cluster("front_rpc_gateway", "127.0.0.1", u["gateway"], tls=rpc_tls),
        cluster("front_rpc_sessions", "127.0.0.1", u["sessions"], tls=rpc_tls),
        cluster("front_rpc_fleet", "127.0.0.1", u["fleet"], tls=rpc_tls),
    ]
    return [main, hyperdx], clusters


def jwks_cluster(url: str) -> dict:
    u = urlsplit(url)
    host = u.hostname or ""
    port = u.port or (443 if u.scheme == "https" else 80)
    tls = {"sni": host} if u.scheme == "https" else None
    return cluster(JWKS_CLUSTER, host, port, h2=False, tls=tls)


def render(spec: Spec, k: Knobs, jwt: Jwt | None, paths: Mapping[str, str]) -> dict:
    """The whole bootstrap. `paths` maps each TLS knob's host path to the path Envoy
    should open (identity for `--mode validate`, the mount point in the container)."""
    p = lambda v: paths.get(v, v)  # noqa: E731
    tls = Tls(p(k.tls_cert), p(k.tls_key)) if k.tls_cert else None
    up = None
    if k.upstream_ca:
        client = Tls(p(k.upstream_cert), p(k.upstream_key)) if k.upstream_cert else None
        up = Upstream(p(k.upstream_ca), k.upstream_sni, client)
    clusters = [
        cluster(l.cluster, "127.0.0.1", l.upstream_port, tls=upstream_tls(up) if up else None)
        for l in spec.listeners
    ]
    clusters.append(cluster("otel_collector", "127.0.0.1", spec.otel_port))
    if jwt is not None and jwt.jwks_url:
        clusters.append(jwks_cluster(jwt.jwks_url))
    listeners = [listener(l, k, jwt, tls) for l in spec.listeners]
    if spec.rest is not None:
        # The transcoder fronts an existing cluster (agent_gateway); no new cluster.
        # `jwt` is the same provider the grpc-web listeners use — edge auth on REST too.
        listeners.append(rest_listener(spec.rest, k, p(spec.rest.descriptor), jwt, tls))
    if spec.webhook is not None:
        # Its own HTTP/1.1 cluster: tiny_http speaks no h2, and no upstream TLS.
        clusters.append(cluster(spec.webhook.cluster, "127.0.0.1", spec.webhook.upstream_port, h2=False))
        listeners.append(webhook_listener(spec.webhook, k, tls))
    if k.front_door == "on" and spec.front_door is not None:
        fls, fcs = front_door(spec.front_door, k, Tls(p(k.front_cert), p(k.front_key)), up)
        listeners += fls
        clusters += fcs
    return {"static_resources": {"listeners": listeners, "clusters": clusters}}


def container_paths(k: Knobs) -> dict[str, str]:
    """Host path → mount point for every TLS file the config names."""
    files = {
        k.tls_cert: "listener.crt", k.tls_key: "listener.key",
        k.upstream_ca: "upstream-ca.crt",
        k.upstream_cert: "upstream.crt", k.upstream_key: "upstream.key",
        k.front_cert: "front.crt", k.front_key: "front.key",
    }
    return {h: f"{CONTAINER_TLS_DIR}/{n}" for h, n in files.items() if h}


def write_private(path: Path, text: str) -> None:
    """The rendered config holds the OTLP ingestion key: owner-readable only."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        f.write(text)
    os.chmod(path, 0o600)


# ---------------------------------------------------------------- commands


def build(args: argparse.Namespace, env: Mapping[str, str], runner: Runner, container: bool):
    spec = Spec.load(json.loads(Path(args.spec).read_text()))
    k = knobs_from_env(env, spec)
    fetch = lambda: fetch_jwks(args.grpcurl, args.proto_dir, k.jwks_from, k.jwks_wait, runner)  # noqa: E731
    jwt = resolve_jwt(k, fetch, lambda p: Path(p).read_text())
    paths = container_paths(k) if container else {}
    if container and spec.rest is not None:
        # Mount the (host store-path) descriptor read-only at a fixed container path, and
        # point the rendered config at that mount — the same host→mount indirection the
        # TLS files use. For `--mode validate` (container=False) the config names the real
        # store path, which is readable in the sandbox.
        paths = {**paths, spec.rest.descriptor: "/etc/envoy/agent_descriptor.pb"}
    return spec, k, jwt, render(spec, k, jwt, paths), paths


def cmd_render(args: argparse.Namespace, env: Mapping[str, str], runner: Runner) -> int:
    _, k, jwt, cfg, _ = build(args, env, runner, container=False)
    write_private(Path(args.out), json.dumps(cfg, indent=2) + "\n")
    for n in k.notes:
        print(f"portal-envoy: note — {n}", file=sys.stderr)
    print(f"portal-envoy: wrote {args.out} (jwt_authn {'on' if jwt else 'off'}, "
          f"front door {k.front_door})")
    return 0


def cmd_up(args: argparse.Namespace, env: Mapping[str, str], runner: Runner) -> int:
    rt = env.get("CONTAINER_RUNTIME", "docker") or "docker"
    if rt not in ("docker", "podman"):
        raise EnvoyError(f"CONTAINER_RUNTIME must be docker or podman, not {rt!r}")
    if runner([rt, "info"]).returncode != 0:
        raise EnvoyError(
            f"'{rt}' not reachable — is it installed/running? "
            "(on a podman-only host: CONTAINER_RUNTIME=podman)")
    spec, k, jwt, cfg, paths = build(args, env, runner, container=True)
    runtime_dir = env.get("XDG_RUNTIME_DIR") or "/tmp"
    effective = Path(runtime_dir) / f"{args.name}-envoy.yaml"
    write_private(effective, json.dumps(cfg, indent=2) + "\n")
    names = runner([rt, "ps", "-a", "--format", "{{.Names}}"]).stdout.split()
    if args.name in names:
        print(f"==> restarting {args.name}")
        runner([rt, "rm", "-f", args.name])
    for n in k.notes:
        print(f"grpc-web-up: note — {n}", file=sys.stderr)
    if not k.otlp_authorization:
        print("grpc-web-up: note — no OTLP ingestion key set (PORTAL_OTLP_AUTHORIZATION / "
              "CLICKSTACK_INGESTION_API_KEY); an auth'd collector drops the bridge's telemetry.",
              file=sys.stderr)
    scheme = "https" if k.tls_cert else "http"
    print(f"==> starting grpc-web proxy ({rt}, {args.image}), jwt_authn {'on' if jwt else 'off'}:")
    for l in spec.listeners:
        print(f"      {scheme}://{k.host}:{l.port}  -> 127.0.0.1:{l.upstream_port} ({l.cluster})")
    if spec.rest is not None:
        r = spec.rest
        print(f"      {scheme}://127.0.0.1:{r.port}  -> 127.0.0.1:{r.upstream_port} "
              f"({r.cluster}, REST/JSON transcoder, {len(r.services)} services)")
    if spec.webhook is not None:
        w = spec.webhook
        print(f"      {scheme}://{k.host}:{w.port}{w.path}  -> 127.0.0.1:{w.upstream_port} "
              f"({w.cluster}, GitHub webhooks, HMAC-verified by the agent, no edge JWT)")
    if k.front_door == "on" and spec.front_door is not None:
        fd = spec.front_door
        print(f"      https://{k.front_host}:{fd.main.port}  front door (portal, RPCs, /grafana, "
              f"/prometheus, /dex), checked by the gate on 127.0.0.1:{fd.upstreams['gate']}")
        print(f"      https://{k.front_host}:{fd.hyperdx.port}  front door (HyperDX), checked the same")
    print(f"      CORS origins: {', '.join(k.origins)}")
    argv = [rt, "run", "-d", "--name", args.name, "--network", "host",
            # the container user must read the 0600 config (and any TLS keys) we own
            "--user", f"{os.getuid()}:{os.getgid()}",
            "-v", f"{effective}:/etc/envoy/envoy.yaml:ro"]
    if rt == "podman":
        argv += ["--userns", "keep-id"]
    for host, mount in paths.items():
        argv += ["-v", f"{host}:{mount}:ro"]
    argv += [args.image, "-c", "/etc/envoy/envoy.yaml"]
    cp = runner(argv)
    if cp.returncode != 0:
        raise EnvoyError(f"{rt} run failed: {(cp.stderr or '').strip()[:400]}")
    print("grpc-web proxy up. Stop with: nix run .#grpc-web-down")
    return 0


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    p = argparse.ArgumentParser(prog="portal-envoy", description=__doc__.split("\n\n")[0])
    p.add_argument("--spec", required=True, help="listener spec JSON (from nix)")
    p.add_argument("--grpcurl", default="grpcurl")
    p.add_argument("--proto-dir", default="", help="the agent's proto root (AuthService.Jwks)")
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("render", help="write the config for `envoy --mode validate`")
    r.add_argument("--out", required=True)
    u = sub.add_parser("up", help="render and (re)start the bridge container")
    u.add_argument("--name", required=True)
    u.add_argument("--image", required=True)
    return p.parse_args(argv)


def main(argv: Sequence[str], env: Mapping[str, str] = os.environ, runner: Runner = run) -> int:
    args = parse_args(argv)
    try:
        return (cmd_render if args.cmd == "render" else cmd_up)(args, env, runner)
    except EnvoyError as e:
        print(f"portal-envoy: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
