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

Design: docs/design/security-hardening/06-portal-and-edge.md.
"""

from __future__ import annotations

import argparse
import ipaddress
import json
import os
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
            return Spec(listeners, int(raw["otel_port"]), int(raw["gateway_port"]), rest)
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
    for path in (k.tls_cert, k.tls_key, k.upstream_ca, k.upstream_cert, k.upstream_key):
        if path and not os.path.isabs(path):
            raise EnvoyError(f"{path!r} must be an absolute path")
    if not is_loopback_host(k.host):
        k.notes.append(
            f"listening on {k.host}: reachable beyond this host; CORS allows only "
            + ", ".join(k.origins)
        )
    return k


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
            "print_options": {"add_whitespace": True, "always_print_primitive_fields": True},
        },
    }


def rest_listener(rest: Rest, k: Knobs, descriptor_path: str, tls: Tls | None) -> dict:
    """The REST/JSON transcoder listener. Filter order `cors ▶ grpc_json_transcoder ▶
    router` (the transcoder must precede the router). No `grpc_web` (this is plain
    HTTP+JSON, not grpc-web framing) and no edge `jwt_authn`: it is pinned to loopback,
    and every transcoded call is an ordinary gRPC call the agent's AuthLayer still
    verifies. Bind is FIXED 127.0.0.1 — it does not follow PORTAL_GRPC_WEB_HOST, so a
    LAN bind of the grpc-web listeners never silently exposes REST."""
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
        "http_filters": [
            {"name": "envoy.filters.http.cors",
             "typed_config": {"@type": any_type("envoy.extensions.filters.http.cors.v3.Cors")}},
            transcoder_filter(descriptor_path, rest.services),
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
        "name": rest.name,
        "address": {"socket_address": {"address": "127.0.0.1", "port_value": rest.port}},
        "filter_chains": [chain],
    }


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
        listeners.append(rest_listener(spec.rest, k, p(spec.rest.descriptor), tls))
    return {"static_resources": {"listeners": listeners, "clusters": clusters}}


def container_paths(k: Knobs) -> dict[str, str]:
    """Host path → mount point for every TLS file the config names."""
    files = {
        k.tls_cert: "listener.crt", k.tls_key: "listener.key",
        k.upstream_ca: "upstream-ca.crt",
        k.upstream_cert: "upstream.crt", k.upstream_key: "upstream.key",
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
    print(f"portal-envoy: wrote {args.out} (jwt_authn {'on' if jwt else 'off'})")
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
