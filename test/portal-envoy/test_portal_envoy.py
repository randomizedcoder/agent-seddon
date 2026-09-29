"""Tests for portal_envoy.py: four-class tables (positive_/negative_/corner_/boundary_
plus adversarial_ for every environment value that reaches the config), a fake
runner for the grpcurl / container calls, and — when PORTAL_ENVOY_BIN and
PORTAL_ENVOY_STEP point at a real envoy and step-cli (the `portal-envoy` nix check
sets both, plus PORTAL_ENVOY_SPEC = the real nix spec) — `envoy --mode validate`
on every rendered mode, plus check-the-checks:
validate must genuinely REJECT a config with a corrupt JWKS, a missing key file and
a rule naming an unknown provider.

Run: python3 -m unittest test_portal_envoy -v
"""

from __future__ import annotations

import io
import json
import os
import subprocess
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

import portal_envoy as E

SPEC = E.Spec(
    listeners=(
        E.Listener("gateway_grpc_web", 8090, "agent_gateway", 50100),
        E.Listener("sessions_grpc_web", 8091, "agent_sessions", 50080),
        E.Listener("fleet_grpc_web", 8093, "agent_fleet", 50086),
    ),
    otel_port=4317,
    gateway_port=50100,
)
SPEC_RAW = {
    "listeners": [
        {"name": l.name, "port": l.port, "cluster": l.cluster, "upstream_port": l.upstream_port}
        for l in SPEC.listeners
    ],
    "otel_port": 4317,
    "gateway_port": 50100,
}
# A public P-256 JWK (no private members): what the agent's AuthService.Jwks serves.
PUBLIC_JWK = {
    "kty": "EC", "crv": "P-256", "alg": "ES256", "use": "sig",
    "kid": "XXzFWbg1C7PBVDFfKAZdfgeMpZ-AjJBIRO182pNXqtI",
    "x": "g6LoJUyVUb2MluBVe-XroT1tc1iFTyn8DD89kjWXcUQ",
    "y": "iRYo0KCRu8IYrQ0qFdLTXPRrcwjnoU9HbB5r_Bt2ZHc",
}
JWKS = json.dumps({"keys": [PUBLIC_JWK]})


def quiet(fn, *a, **kw):
    with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
        return fn(*a, **kw)


def cp(rc: int = 0, out: str = "", err: str = "") -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess([], rc, out, err)


class FakeRunner:
    """Answers each call from a queue of results (the last one repeats)."""

    def __init__(self, *results: subprocess.CompletedProcess):
        self.results = list(results) or [cp()]
        self.calls: list[list[str]] = []

    def __call__(self, argv):
        self.calls.append(list(argv))
        return self.results.pop(0) if len(self.results) > 1 else self.results[0]


def hcm(cfg: dict, i: int = 0) -> dict:
    return cfg["static_resources"]["listeners"][i]["filter_chains"][0]["filters"][0]["typed_config"]


def filter_names(cfg: dict, i: int = 0) -> list[str]:
    return [f["name"] for f in hcm(cfg, i)["http_filters"]]


def rendered(env: dict, jwt: E.Jwt | None = None, paths: dict | None = None) -> dict:
    return E.render(SPEC, E.knobs_from_env(env, SPEC), jwt, paths or {})


class ValidateOrigin(unittest.TestCase):
    CASES = [
        ("positive_http_loopback_port", "http://127.0.0.1:8092", "http://127.0.0.1:8092"),
        ("positive_https_name", "https://portal.example.com", "https://portal.example.com"),
        ("corner_trailing_slash_dropped", "https://portal.example.com/", "https://portal.example.com"),
        ("corner_case_normalised", "HTTP://Portal.Example.COM:8092", "http://portal.example.com:8092"),
        ("corner_ipv6_literal", "http://[::1]:8092", "http://[::1]:8092"),
        ("boundary_port_65535", "http://a.example:65535", "http://a.example:65535"),
        ("boundary_port_65536", "http://a.example:65536", None),
        ("negative_empty", "", None),
        ("negative_no_scheme", "portal.example.com", None),
        ("negative_ftp_scheme", "ftp://portal.example.com", None),
        ("negative_path", "https://portal.example.com/app", None),
        ("negative_query", "https://portal.example.com?x=1", None),
        ("adversarial_wildcard", "*", None),
        ("adversarial_wildcard_subdomain", "https://*.example.com", None),
        ("adversarial_null_origin", "null", None),
        ("adversarial_credentials", "https://user:pw@portal.example.com", None),
        ("adversarial_newline_injection", "https://a.example\nx-evil: 1", None),
        ("adversarial_javascript_scheme", "javascript://portal.example.com", None),
    ]

    def test_table(self):
        for name, value, want in self.CASES:
            with self.subTest(name):
                if want is None:
                    with self.assertRaises(E.EnvoyError):
                        E.validate_origin(value)
                else:
                    self.assertEqual(E.validate_origin(value), want)

    def test_list(self):
        cases = [
            ("positive_two", "http://a.example, http://b.example", ("http://a.example", "http://b.example")),
            ("corner_duplicates_collapse", "http://a.example,HTTP://A.example/", ("http://a.example",)),
            ("corner_empty_items_skipped", ",http://a.example,,", ("http://a.example",)),
            ("boundary_sixteen", ",".join(f"http://h{i}.example" for i in range(16)), None),
            ("boundary_seventeen_rejected", ",".join(f"http://h{i}.example" for i in range(17)), "raise"),
            ("negative_only_commas", ",,", "raise"),
            ("adversarial_one_bad_poisons_all", "http://a.example,*", "raise"),
        ]
        for name, value, want in cases:
            with self.subTest(name):
                if want == "raise":
                    with self.assertRaises(E.EnvoyError):
                        E.validate_origins(value)
                elif want is None:
                    self.assertEqual(len(E.validate_origins(value)), 16)
                else:
                    self.assertEqual(E.validate_origins(value), want)


class ValidateHost(unittest.TestCase):
    CASES = [
        ("positive_loopback", "127.0.0.1", "127.0.0.1"),
        ("positive_all_interfaces", "0.0.0.0", "0.0.0.0"),
        ("positive_ipv6_loopback", "::1", "::1"),
        ("corner_whitespace_trimmed", " 10.0.0.5 ", "10.0.0.5"),
        ("negative_name", "localhost", None),
        ("negative_empty", "", None),
        ("boundary_octet_256", "10.0.0.256", None),
        ("adversarial_injection", "127.0.0.1\n  port_value: 1", None),
        ("adversarial_host_port", "127.0.0.1:80", None),
    ]

    def test_table(self):
        for name, value, want in self.CASES:
            with self.subTest(name):
                if want is None:
                    with self.assertRaises(E.EnvoyError):
                        E.validate_host(value)
                else:
                    self.assertEqual(E.validate_host(value), want)


class ValidateJwks(unittest.TestCase):
    def test_table(self):
        big = json.dumps({"keys": [dict(PUBLIC_JWK, pad="x" * E.MAX_JWKS_BYTES)]})
        cases = [
            ("positive_one_key", JWKS, True),
            ("positive_two_keys_rotation", json.dumps({"keys": [PUBLIC_JWK, dict(PUBLIC_JWK, kid="old")]}), True),
            ("negative_not_json", "{keys", False),
            ("negative_empty_keys", '{"keys":[]}', False),
            ("negative_no_keys_member", "{}", False),
            ("negative_array_document", "[]", False),
            ("corner_key_without_kty", '{"keys":[{"kid":"a"}]}', False),
            ("boundary_oversized", big, False),
            ("adversarial_private_ec_member", json.dumps({"keys": [dict(PUBLIC_JWK, d="secret")]}), False),
            ("adversarial_symmetric_key", '{"keys":[{"kty":"oct","k":"c2VjcmV0"}]}', False),
        ]
        for name, doc, ok in cases:
            with self.subTest(name):
                if ok:
                    self.assertEqual(json.loads(E.validate_jwks(doc))["keys"][0]["kty"], "EC")
                else:
                    with self.assertRaises(E.EnvoyError):
                        E.validate_jwks(doc)


class ValidateJwksUrl(unittest.TestCase):
    CASES = [
        ("positive_https", "https://agent.example/.well-known/jwks.json", True),
        ("positive_http_loopback", "http://127.0.0.1:50100/v1/auth/jwks", True),
        ("corner_http_ipv6_loopback", "http://[::1]:8080/jwks", True),
        ("negative_http_remote", "http://agent.example/jwks", False),
        ("negative_no_host", "https:///jwks", False),
        ("adversarial_http_localhost_name", "http://localhost/jwks", False),
        ("adversarial_credentials", "https://u:p@agent.example/jwks", False),
        ("adversarial_file_scheme", "file:///etc/passwd", False),
        ("adversarial_bad_port", "https://agent.example:99999/jwks", False),
    ]

    def test_table(self):
        for name, value, ok in self.CASES:
            with self.subTest(name):
                if ok:
                    self.assertEqual(E.validate_jwks_url(value), value)
                else:
                    with self.assertRaises(E.EnvoyError):
                        E.validate_jwks_url(value)


class KnobsFromEnv(unittest.TestCase):
    def test_defaults_are_the_safe_ones(self):
        k = E.knobs_from_env({}, SPEC)
        self.assertEqual(k.host, "127.0.0.1")
        self.assertEqual(k.origins, E.DEFAULT_ORIGINS)
        self.assertEqual(k.auth, "auto")
        self.assertEqual(k.jwks_from, "127.0.0.1:50100")
        self.assertEqual(k.notes, [])

    def test_table(self):
        cases = [
            ("positive_lan_bind_notes_it", {"PORTAL_GRPC_WEB_HOST": "0.0.0.0"}, "note"),
            ("positive_auth_off", {"PORTAL_AUTH": "off"}, None),
            ("positive_full_mtls", {"PORTAL_UPSTREAM_CA": "/ca", "PORTAL_UPSTREAM_CERT": "/c", "PORTAL_UPSTREAM_KEY": "/k"}, None),
            ("negative_unknown_auth_mode", {"PORTAL_AUTH": "maybe"}, "raise"),
            ("negative_tls_cert_without_key", {"PORTAL_TLS_CERT": "/c"}, "raise"),
            ("negative_upstream_key_without_cert", {"PORTAL_UPSTREAM_CA": "/ca", "PORTAL_UPSTREAM_KEY": "/k"}, "raise"),
            ("negative_client_cert_without_ca", {"PORTAL_UPSTREAM_CERT": "/c", "PORTAL_UPSTREAM_KEY": "/k"}, "raise"),
            ("negative_relative_tls_path", {"PORTAL_TLS_CERT": "c.pem", "PORTAL_TLS_KEY": "/k"}, "raise"),
            ("boundary_wait_max", {"PORTAL_JWKS_WAIT": str(E.MAX_JWKS_WAIT)}, None),
            ("boundary_wait_over_max", {"PORTAL_JWKS_WAIT": str(E.MAX_JWKS_WAIT + 1)}, "raise"),
            ("corner_wait_zero", {"PORTAL_JWKS_WAIT": "0"}, None),
            ("adversarial_negative_wait", {"PORTAL_JWKS_WAIT": "-1"}, "raise"),
            ("adversarial_otlp_key_crlf", {"PORTAL_OTLP_AUTHORIZATION": "k\r\nx-evil: 1"}, "raise"),
            ("adversarial_issuer_newline", {"PORTAL_JWT_ISSUER": "https://a\n"}, None),  # stripped
            ("adversarial_issuer_inner_control", {"PORTAL_JWT_ISSUER": "https://a\x00b"}, "raise"),
            ("adversarial_jwks_from_option", {"PORTAL_JWKS_FROM": "-plaintext:1"}, "raise"),
            ("adversarial_jwks_from_no_port", {"PORTAL_JWKS_FROM": "agent.example"}, "raise"),
            ("adversarial_sni_wildcard", {"PORTAL_UPSTREAM_SNI": "*.example"}, "raise"),
        ]
        for name, env, want in cases:
            with self.subTest(name):
                if want == "raise":
                    with self.assertRaises(E.EnvoyError):
                        E.knobs_from_env(env, SPEC)
                else:
                    k = E.knobs_from_env(env, SPEC)
                    self.assertEqual(bool(k.notes), want == "note")


class FetchJwks(unittest.TestCase):
    def fetch(self, runner, wait=0):
        return E.fetch_jwks("grpcurl", "/protos", "127.0.0.1:50100", wait, runner, sleep=lambda _: None)

    def test_table(self):
        ok = cp(0, json.dumps({"jwksJson": JWKS}))
        refused = cp(1, err="Failed to dial target host: connection refused")
        cases = [
            ("positive_keys", FakeRunner(ok), JWKS),
            ("negative_unimplemented_means_no_tokens", FakeRunner(cp(1, err="ERROR:\n  Code: Unimplemented")), None),
            ("corner_empty_document_means_no_tokens", FakeRunner(cp(0, "{}")), None),
            ("corner_empty_key_set_means_no_tokens", FakeRunner(cp(0, json.dumps({"jwksJson": '{"keys":[]}'}))), None),
            ("negative_unreachable_fails_closed", FakeRunner(refused), "raise"),
            ("adversarial_non_json_answer", FakeRunner(cp(0, "<html>")), "raise"),
        ]
        for name, runner, want in cases:
            with self.subTest(name):
                if want == "raise":
                    with self.assertRaises(E.EnvoyError):
                        self.fetch(runner)
                else:
                    self.assertEqual(self.fetch(runner), want)

    def test_boundary_retries_until_the_agent_answers(self):
        runner = FakeRunner(cp(1, err="connection refused"), cp(1, err="connection refused"),
                            cp(0, json.dumps({"jwksJson": JWKS})))
        self.assertEqual(self.fetch(runner, wait=60), JWKS)
        self.assertEqual(len(runner.calls), 3)

    def test_positive_argv_uses_the_committed_proto_not_reflection(self):
        runner = FakeRunner(cp(0, json.dumps({"jwksJson": JWKS})))
        self.fetch(runner)
        argv = runner.calls[0]
        self.assertIn("-proto", argv)
        self.assertEqual(argv[-2:], ["127.0.0.1:50100", "agent.v1.AuthService/Jwks"])


class ResolveJwt(unittest.TestCase):
    def resolve(self, env, fetched=None, files=None):
        k = E.knobs_from_env(env, SPEC)
        calls = []

        def fetch():
            calls.append(1)
            if isinstance(fetched, Exception):
                raise fetched
            return fetched

        def read(p):
            if files and p in files:
                return files[p]
            raise FileNotFoundError(2, "No such file or directory")

        return E.resolve_jwt(k, fetch, read), k, calls

    def test_table(self):
        cases = [
            ("positive_auto_with_agent_tokens", {}, JWKS, None, "inline"),
            ("positive_on_with_file", {"PORTAL_AUTH": "on", "PORTAL_JWT_JWKS": "/j.json"}, None, {"/j.json": JWKS}, "inline"),
            ("positive_on_with_url", {"PORTAL_AUTH": "on", "PORTAL_JWT_JWKS": "https://a.example/jwks"}, None, None, "url"),
            ("positive_off_never_fetches", {"PORTAL_AUTH": "off"}, JWKS, None, None),
            ("corner_auto_without_agent_tokens_renders_none", {}, None, None, None),
            ("negative_on_without_agent_tokens_refuses", {"PORTAL_AUTH": "on"}, None, None, "raise"),
            ("negative_missing_file", {"PORTAL_JWT_JWKS": "/nope.json"}, None, None, "raise"),
            ("negative_relative_file", {"PORTAL_JWT_JWKS": "j.json"}, None, None, "raise"),
            ("negative_unreachable_agent_fails_closed", {}, E.EnvoyError("down"), None, "raise"),
            ("adversarial_private_key_file", {"PORTAL_JWT_JWKS": "/j.json"}, None,
             {"/j.json": json.dumps({"keys": [dict(PUBLIC_JWK, d="x")]})}, "raise"),
            ("adversarial_http_remote_url", {"PORTAL_JWT_JWKS": "http://a.example/jwks"}, None, None, "raise"),
        ]
        for name, env, fetched, files, want in cases:
            with self.subTest(name):
                if want == "raise":
                    with self.assertRaises(E.EnvoyError):
                        self.resolve(env, fetched, files)
                    continue
                jwt, k, calls = self.resolve(env, fetched, files)
                if want is None:
                    self.assertIsNone(jwt)
                    self.assertTrue(k.notes, "an edge without jwt_authn is always noted")
                elif want == "inline":
                    self.assertIsNotNone(jwt.jwks_inline)
                else:
                    self.assertTrue(jwt.jwks_url.startswith("https://"))
                if env.get("PORTAL_AUTH") == "off" or env.get("PORTAL_JWT_JWKS"):
                    self.assertEqual(calls, [], "no fetch when off or given a JWKS")


class Render(unittest.TestCase):
    JWT = E.Jwt("https://agent.example", ("agent-seddon",), jwks_inline=E.validate_jwks(JWKS))

    def test_positive_every_listener_binds_the_host(self):
        for env, want in (({}, "127.0.0.1"), ({"PORTAL_GRPC_WEB_HOST": "0.0.0.0"}, "0.0.0.0")):
            cfg = rendered(env)
            binds = {l["address"]["socket_address"]["address"] for l in cfg["static_resources"]["listeners"]}
            self.assertEqual(binds, {want})

    def test_positive_cors_is_exact_and_allows_authorization(self):
        cfg = rendered({"PORTAL_WEB_ORIGIN": "https://portal.example.com"})
        for i in range(len(SPEC.listeners)):
            policy = hcm(cfg, i)["route_config"]["virtual_hosts"][0]["typed_per_filter_config"]["envoy.filters.http.cors"]
            self.assertEqual(policy["allow_origin_string_match"], [{"exact": "https://portal.example.com"}])
            self.assertIn("authorization", policy["allow_headers"].split(","))

    def test_negative_no_prefix_or_wildcard_origin_anywhere(self):
        text = json.dumps(rendered({}))
        self.assertNotIn('"prefix": "*"', text)
        self.assertNotIn('"0.0.0.0"', text)

    def test_positive_jwt_filter_sits_between_cors_and_router(self):
        cfg = rendered({}, self.JWT)
        for i in range(len(SPEC.listeners)):
            self.assertEqual(filter_names(cfg, i), [
                "envoy.filters.http.grpc_web", "envoy.filters.http.cors",
                "envoy.filters.http.jwt_authn", "envoy.filters.http.router"])

    def test_corner_no_jwt_renders_no_filter(self):
        self.assertNotIn("jwt_authn", json.dumps(rendered({})))

    def test_positive_jwt_rules_bypass_auth_health_reflection_and_require_the_rest(self):
        jwt = hcm(rendered({}, self.JWT))["http_filters"][2]["typed_config"]
        rules = jwt["rules"]
        self.assertEqual([r["match"]["prefix"] for r in rules[:-1]], list(E.UNAUTHENTICATED_PREFIXES))
        self.assertTrue(all("requires" not in r for r in rules[:-1]))
        self.assertEqual(rules[-1], {"match": {"prefix": "/"}, "requires": {"provider_name": "agent"}})
        provider = jwt["providers"]["agent"]
        self.assertTrue(provider["forward"], "the agent must still see the bearer")
        self.assertEqual(provider["issuer"], "https://agent.example")
        self.assertEqual(provider["audiences"], ["agent-seddon"])
        self.assertTrue(jwt["bypass_cors_preflight"])

    def test_corner_unset_issuer_audience_are_omitted_not_empty(self):
        jwt = E.Jwt("", (), jwks_inline=E.validate_jwks(JWKS))
        provider = hcm(rendered({}, jwt))["http_filters"][2]["typed_config"]["providers"]["agent"]
        self.assertNotIn("issuer", provider)
        self.assertNotIn("audiences", provider)

    def test_positive_remote_jwks_gets_a_tls_cluster(self):
        jwt = E.Jwt("", (), jwks_url="https://agent.example:8443/jwks")
        clusters = {c["name"]: c for c in rendered({}, jwt)["static_resources"]["clusters"]}
        c = clusters[E.JWKS_CLUSTER]
        addr = c["load_assignment"]["endpoints"][0]["lb_endpoints"][0]["endpoint"]["address"]["socket_address"]
        self.assertEqual((addr["address"], addr["port_value"]), ("agent.example", 8443))
        self.assertEqual(c["transport_socket"]["typed_config"]["sni"], "agent.example")

    def test_positive_downstream_and_upstream_tls(self):
        env = {"PORTAL_TLS_CERT": "/l.crt", "PORTAL_TLS_KEY": "/l.key", "PORTAL_UPSTREAM_CA": "/ca",
               "PORTAL_UPSTREAM_CERT": "/u.crt", "PORTAL_UPSTREAM_KEY": "/u.key", "PORTAL_UPSTREAM_SNI": "agent"}
        cfg = rendered(env)
        chain = cfg["static_resources"]["listeners"][0]["filter_chains"][0]
        certs = chain["transport_socket"]["typed_config"]["common_tls_context"]["tls_certificates"][0]
        self.assertEqual(certs["certificate_chain"]["filename"], "/l.crt")
        agent = {c["name"]: c for c in cfg["static_resources"]["clusters"]}["agent_gateway"]
        up = agent["transport_socket"]["typed_config"]
        self.assertEqual(up["sni"], "agent")
        vc = up["common_tls_context"]["validation_context"]
        self.assertEqual(vc["match_typed_subject_alt_names"], [{"san_type": "DNS", "matcher": {"exact": "agent"}}])
        self.assertEqual(up["common_tls_context"]["tls_certificates"][0]["private_key"]["filename"], "/u.key")
        otel = {c["name"]: c for c in cfg["static_resources"]["clusters"]}["otel_collector"]
        self.assertNotIn("transport_socket", otel, "the collector is not the agent")

    def test_corner_upstream_tls_without_client_cert(self):
        up = {c["name"]: c for c in rendered({"PORTAL_UPSTREAM_CA": "/ca"})["static_resources"]["clusters"]}
        ctx = up["agent_fleet"]["transport_socket"]["typed_config"]["common_tls_context"]
        self.assertNotIn("tls_certificates", ctx)

    def test_positive_container_paths_remap_every_tls_file(self):
        k = E.knobs_from_env({"PORTAL_TLS_CERT": "/h/l.crt", "PORTAL_TLS_KEY": "/h/l.key"}, SPEC)
        paths = E.container_paths(k)
        text = json.dumps(E.render(SPEC, k, None, paths))
        self.assertNotIn("/h/l.key", text)
        self.assertIn(f"{E.CONTAINER_TLS_DIR}/listener.key", text)

    def test_boundary_otlp_key_lands_only_in_otlp_metadata(self):
        cfg = rendered({"PORTAL_OTLP_AUTHORIZATION": "sekrit"})
        text = json.dumps(cfg)
        # access log + tracer on each of three listeners
        self.assertEqual(text.count('"sekrit"'), 2 * len(SPEC.listeners))

    def test_adversarial_values_cannot_escape_their_string(self):
        # A quote or brace in a free-form value stays inside its JSON string.
        jwt = E.Jwt('x", "evil": {"a', ("aud\"}",), jwks_inline=E.validate_jwks(JWKS))
        cfg = json.loads(json.dumps(rendered({}, jwt)))
        provider = hcm(cfg)["http_filters"][2]["typed_config"]["providers"]["agent"]
        self.assertEqual(provider["issuer"], 'x", "evil": {"a')
        self.assertNotIn("evil", provider)


class Commands(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.spec = self.dir / "spec.json"
        self.spec.write_text(json.dumps(SPEC_RAW))

    def tearDown(self):
        self.tmp.cleanup()

    def test_positive_render_writes_owner_only(self):
        out = self.dir / "cfg.json"
        rc = quiet(E.main, ["--spec", str(self.spec), "render", "--out", str(out)], {"PORTAL_AUTH": "off"})
        self.assertEqual(rc, 0)
        self.assertEqual(out.stat().st_mode & 0o777, 0o600)
        self.assertIn("static_resources", json.loads(out.read_text()))

    def test_negative_bad_knob_exits_2_and_writes_nothing(self):
        out = self.dir / "cfg.json"
        rc = quiet(E.main, ["--spec", str(self.spec), "render", "--out", str(out)], {"PORTAL_WEB_ORIGIN": "*"})
        self.assertEqual(rc, 2)
        self.assertFalse(out.exists())

    def test_negative_bad_spec(self):
        self.spec.write_text('{"listeners":[{"name":"x"}]}')
        rc = quiet(E.main, ["--spec", str(self.spec), "render", "--out", str(self.dir / "c")], {"PORTAL_AUTH": "off"})
        self.assertEqual(rc, 2)

    def up(self, env, *results):
        runner = FakeRunner(*results)
        env = {"XDG_RUNTIME_DIR": str(self.dir), "PORTAL_AUTH": "off", **env}
        rc = quiet(E.main, ["--spec", str(self.spec), "up", "--name", "bridge", "--image", "envoy:x"], env, runner)
        return rc, runner

    def test_positive_up_podman_mounts_config_and_tls(self):
        rc, runner = self.up({"CONTAINER_RUNTIME": "podman", "PORTAL_TLS_CERT": "/h/l.crt", "PORTAL_TLS_KEY": "/h/l.key"},
                             cp(0), cp(0, "other\n"), cp(0))
        self.assertEqual(rc, 0)
        run = runner.calls[-1]
        self.assertEqual(run[:2], ["podman", "run"])
        self.assertIn("--userns", run)
        self.assertIn(f"{self.dir}/bridge-envoy.yaml:/etc/envoy/envoy.yaml:ro", run)
        self.assertIn(f"/h/l.key:{E.CONTAINER_TLS_DIR}/listener.key:ro", run)
        self.assertEqual((self.dir / "bridge-envoy.yaml").stat().st_mode & 0o777, 0o600)
        self.assertFalse(any(c[1:3] == ["rm", "-f"] for c in runner.calls), "nothing to replace")

    def test_corner_up_replaces_a_running_bridge(self):
        rc, runner = self.up({}, cp(0), cp(0, "bridge\n"), cp(0))
        self.assertEqual(rc, 0)
        self.assertIn(["docker", "rm", "-f", "bridge"], runner.calls)
        self.assertNotIn("--userns", runner.calls[-1])

    def test_negative_runtime_unreachable(self):
        rc, runner = self.up({}, cp(1))
        self.assertEqual(rc, 2)
        self.assertEqual(len(runner.calls), 1)

    def test_negative_container_run_failure_is_reported(self):
        rc, _ = self.up({}, cp(0), cp(0, ""), cp(125, err="port in use"))
        self.assertEqual(rc, 2)

    def test_adversarial_runtime_name(self):
        rc, runner = self.up({"CONTAINER_RUNTIME": "sh -c id"})
        self.assertEqual(rc, 2)
        self.assertEqual(runner.calls, [])

    def test_negative_up_refuses_before_touching_a_running_bridge(self):
        # A bad knob must not tear down the bridge that is already serving.
        rc, runner = self.up({"PORTAL_WEB_ORIGIN": "*"}, cp(0), cp(0, "bridge\n"))
        self.assertEqual(rc, 2)
        self.assertFalse(any("rm" in c for c in runner.calls))


@unittest.skipUnless(os.environ.get("PORTAL_ENVOY_BIN") and os.environ.get("PORTAL_ENVOY_STEP"),
                     "needs PORTAL_ENVOY_BIN + PORTAL_ENVOY_STEP (the portal-envoy nix check)")
class EnvoyValidate(unittest.TestCase):
    """The rendered configs as Envoy itself reads them."""

    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        d = cls.dir = Path(cls.tmp.name)
        step = os.environ["PORTAL_ENVOY_STEP"]

        def mint(*args):
            subprocess.run([step, "certificate", "create", *args, "--no-password", "--insecure", "--force"],
                           check=True, capture_output=True)

        mint("dev root", str(d / "ca.crt"), str(d / "ca.key"), "--profile", "root-ca")
        mint("localhost", str(d / "leaf.crt"), str(d / "leaf.key"), "--profile", "leaf",
             "--ca", str(d / "ca.crt"), "--ca-key", str(d / "ca.key"), "--san", "localhost")
        (d / "jwks.json").write_text(JWKS)
        # The real spec nix renders (PORTAL_ENVOY_SPEC), else this file's mirror of it.
        cls.spec = d / "spec.json"
        real = os.environ.get("PORTAL_ENVOY_SPEC")
        cls.spec.write_text(Path(real).read_text() if real else json.dumps(SPEC_RAW))

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def validate(self, path: Path) -> subprocess.CompletedProcess:
        return subprocess.run(
            [os.environ["PORTAL_ENVOY_BIN"], "--mode", "validate", "--disable-hot-restart", "-c", str(path)],
            capture_output=True, text=True)

    def render(self, name: str, env: dict) -> Path:
        out = self.dir / f"{name}.json"
        rc = quiet(E.main, ["--spec", str(self.spec), "render", "--out", str(out)], env)
        self.assertEqual(rc, 0, name)
        return out

    def modes(self) -> dict[str, dict]:
        d = self.dir
        tls = {"PORTAL_TLS_CERT": str(d / "leaf.crt"), "PORTAL_TLS_KEY": str(d / "leaf.key"),
               "PORTAL_UPSTREAM_CA": str(d / "ca.crt"), "PORTAL_UPSTREAM_CERT": str(d / "leaf.crt"),
               "PORTAL_UPSTREAM_KEY": str(d / "leaf.key")}
        on = {"PORTAL_AUTH": "on", "PORTAL_JWT_JWKS": str(d / "jwks.json"),
              "PORTAL_JWT_ISSUER": "https://agent.example", "PORTAL_JWT_AUDIENCE": "agent-seddon"}
        return {
            "positive_auth_off": {"PORTAL_AUTH": "off"},
            "positive_auth_on_local_jwks": on,
            "positive_auth_on_remote_jwks": {"PORTAL_AUTH": "on", "PORTAL_JWT_JWKS": "https://agent.example/jwks"},
            "positive_lan_bind_two_origins": {"PORTAL_AUTH": "off", "PORTAL_GRPC_WEB_HOST": "0.0.0.0",
                                              "PORTAL_WEB_ORIGIN": "http://10.0.0.5:8092,https://portal.example"},
            "positive_tls_and_upstream_mtls_with_auth": {**on, **tls},
        }

    def test_every_mode_validates(self):
        for name, env in self.modes().items():
            with self.subTest(name):
                cp_ = self.validate(self.render(name, env))
                self.assertEqual(cp_.returncode, 0, cp_.stderr[-600:])

    def test_check_the_checks_validate_rejects_broken_configs(self):
        base = json.loads(self.render("base", self.modes()["positive_tls_and_upstream_mtls_with_auth"]).read_text())

        def jwt_of(cfg):
            return cfg["static_resources"]["listeners"][0]["filter_chains"][0]["filters"][0]["typed_config"]["http_filters"][2]["typed_config"]

        def corrupt_jwks(cfg):
            jwt_of(cfg)["providers"]["agent"]["local_jwks"]["inline_string"] = '{"keys":[{"kty":"EC","crv":"P-256","x":"zz","y":"zz"}]}'

        def unknown_provider(cfg):
            jwt_of(cfg)["rules"][-1]["requires"]["provider_name"] = "nobody"

        def missing_key(cfg):
            chain = cfg["static_resources"]["listeners"][0]["filter_chains"][0]
            chain["transport_socket"]["typed_config"]["common_tls_context"]["tls_certificates"][0]["private_key"]["filename"] = str(self.dir / "nope.key")

        for name, breaker in (("adversarial_corrupt_jwks", corrupt_jwks),
                              ("negative_unknown_provider", unknown_provider),
                              ("negative_missing_key_file", missing_key)):
            with self.subTest(name):
                cfg = json.loads(json.dumps(base))
                breaker(cfg)
                path = self.dir / f"{name}.json"
                path.write_text(json.dumps(cfg))
                self.assertNotEqual(self.validate(path).returncode, 0, f"{name} must fail validate")


REST = E.Rest("rest_transcoder", 8094, "agent_gateway", 50100, "/nix/store/desc.pb",
              ("agent.v1.AuthService", "agent.v1.ReviewFleetService"))
SPEC_REST = E.Spec(listeners=SPEC.listeners, otel_port=4317, gateway_port=50100, rest=REST)


def rest_listener_of(cfg: dict) -> dict:
    return next(l for l in cfg["static_resources"]["listeners"] if l["name"] == "rest_transcoder")


def rest_hcm(cfg: dict) -> dict:
    return rest_listener_of(cfg)["filter_chains"][0]["filters"][0]["typed_config"]


def rest_transcoder_of(cfg: dict) -> dict:
    return next(f for f in rest_hcm(cfg)["http_filters"]
               if f["name"].endswith("grpc_json_transcoder"))["typed_config"]


def rest_jwt_of(cfg: dict) -> dict:
    return next(f for f in rest_hcm(cfg)["http_filters"]
               if f["name"].endswith("jwt_authn"))["typed_config"]


class LoadServices(unittest.TestCase):
    """The transcoder service list is derived from the descriptor build; the renderer
    fails closed on anything that is not a clean, populated agent.v1 list (untrusted
    file shape). Each case: description, file contents, expected."""

    def write(self, text: str) -> str:
        f = tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False)
        f.write(text)
        f.close()
        self.addCleanup(lambda: os.unlink(f.name))
        return f.name

    def test_positive_reads_sorts_and_dedups(self):
        got = E.load_services(self.write("agent.v1.B\nagent.v1.A\nagent.v1.A\n"))
        self.assertEqual(got, ("agent.v1.A", "agent.v1.B"))

    def test_corner_blank_lines_and_whitespace_are_ignored(self):
        got = E.load_services(self.write("\n  agent.v1.A  \n\n"))
        self.assertEqual(got, ("agent.v1.A",))

    def test_negative_missing_file(self):
        with self.assertRaises(E.EnvoyError):
            E.load_services("/no/such/services.txt")

    def test_adversarial_empty_file(self):
        with self.assertRaises(E.EnvoyError):
            E.load_services(self.write("\n   \n"))

    def test_adversarial_non_agent_name(self):
        for bad in ("google.api.Http", "agent.v2.X", "agent.Foo", "ReviewFleetService", "agent.v1."):
            with self.subTest(bad), self.assertRaises(E.EnvoyError):
                E.load_services(self.write(f"agent.v1.Good\n{bad}\n"))

    def test_adversarial_control_character(self):
        with self.assertRaises(E.EnvoyError):
            E.load_services(self.write("agent.v1.A\x00Evil\n"))

    def test_adversarial_too_many_services(self):
        many = "\n".join(f"agent.v1.S{i}" for i in range(E.MAX_SERVICES + 1))
        with self.assertRaises(E.EnvoyError):
            E.load_services(self.write(many))


class SpecLoadRest(unittest.TestCase):
    def raw_with_rest(self, services_file: str) -> dict:
        return {**SPEC_RAW, "rest": {
            "name": "rest_transcoder", "port": 8094, "cluster": "agent_gateway",
            "upstream_port": 50100, "descriptor": "/nix/store/desc.pb",
            "services_file": services_file}}

    def test_positive_rest_is_parsed(self):
        f = tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False)
        f.write("agent.v1.AuthService\n")
        f.close()
        self.addCleanup(lambda: os.unlink(f.name))
        spec = E.Spec.load(self.raw_with_rest(f.name))
        self.assertEqual(spec.rest.name, "rest_transcoder")
        self.assertEqual(spec.rest.services, ("agent.v1.AuthService",))

    def test_boundary_no_rest_key_is_backward_compatible(self):
        self.assertIsNone(E.Spec.load(SPEC_RAW).rest)

    def test_negative_bad_rest_services_file_fails_the_whole_spec(self):
        with self.assertRaises(E.EnvoyError):
            E.Spec.load(self.raw_with_rest("/no/such/services.txt"))


class RestTranscoder(unittest.TestCase):
    JWT = E.Jwt("https://agent.example", ("agent-seddon",), jwks_inline=E.validate_jwks(JWKS))

    def render(self, env: dict, jwt: E.Jwt | None = None, paths: dict | None = None) -> dict:
        return E.render(SPEC_REST, E.knobs_from_env(env, SPEC_REST), jwt, paths or {})

    def test_positive_rest_listener_is_appended_after_the_grpc_web_ones(self):
        cfg = self.render({})
        names = [l["name"] for l in cfg["static_resources"]["listeners"]]
        self.assertEqual(names[-1], "rest_transcoder")
        self.assertEqual(len(names), len(SPEC.listeners) + 1)

    def test_positive_filter_order_is_cors_transcoder_router(self):
        names = [f["name"] for f in rest_hcm(self.render({}))["http_filters"]]
        self.assertEqual(names, [
            "envoy.filters.http.cors",
            "envoy.filters.http.grpc_json_transcoder",
            "envoy.filters.http.router"])

    def test_positive_transcoder_config_is_fail_closed(self):
        tc = rest_transcoder_of(self.render({}))
        self.assertEqual(tc["proto_descriptor"], "/nix/store/desc.pb")
        self.assertEqual(tc["services"], ["agent.v1.AuthService", "agent.v1.ReviewFleetService"])
        self.assertFalse(tc["auto_mapping"])
        self.assertTrue(tc["match_incoming_request_route"])
        self.assertTrue(tc["convert_grpc_status"])
        self.assertTrue(tc["request_validation_options"]["reject_unknown_method"])
        self.assertTrue(tc["request_validation_options"]["reject_unknown_query_parameters"])

    def test_positive_transcoder_emits_compact_json(self):
        # `add_whitespace: false` keeps transcoded bodies compact — pretty-printing nearly
        # doubles a config read on the wire (49% measured) for no machine-client benefit.
        # `always_print_primitive_fields` stays true: an API-shape contract, not formatting.
        po = rest_transcoder_of(self.render({}))["print_options"]
        self.assertIs(po["add_whitespace"], False)
        self.assertIs(po["always_print_primitive_fields"], True)

    def test_positive_cors_allows_authorization_on_the_rest_listener(self):
        policy = rest_hcm(self.render({}))["route_config"]["virtual_hosts"][0][
            "typed_per_filter_config"]["envoy.filters.http.cors"]
        self.assertIn("authorization", policy["allow_headers"].split(","))

    def test_positive_rest_stays_loopback_even_when_grpc_web_binds_lan(self):
        cfg = self.render({"PORTAL_GRPC_WEB_HOST": "0.0.0.0"})
        self.assertEqual(rest_listener_of(cfg)["address"]["socket_address"]["address"], "127.0.0.1")
        gw = cfg["static_resources"]["listeners"][0]["address"]["socket_address"]["address"]
        self.assertEqual(gw, "0.0.0.0", "the grpc-web listener still honours the host knob")

    def test_positive_edge_jwt_on_rest_sits_after_the_transcoder(self):
        # With auth on, the REST listener gains jwt_authn — AFTER grpc_json_transcoder,
        # so the transcoder's :path rewrite lets it reuse the gRPC UNAUTHENTICATED_PREFIXES.
        names = [f["name"] for f in rest_hcm(self.render({}, self.JWT))["http_filters"]]
        self.assertEqual(names, [
            "envoy.filters.http.cors",
            "envoy.filters.http.grpc_json_transcoder",
            "envoy.filters.http.jwt_authn",
            "envoy.filters.http.router"])

    def test_corner_no_edge_jwt_on_rest_when_auth_off(self):
        # Auth off (jwt is None): no edge check; the agent's AuthLayer still verifies.
        names = [f["name"] for f in rest_hcm(self.render({}))["http_filters"]]
        self.assertNotIn("envoy.filters.http.jwt_authn", names)

    def test_positive_rest_jwt_reuses_grpc_prefixes_and_forwards(self):
        # One source of truth: the REST edge check reuses the same exempt prefixes as
        # the grpc-web listeners (the transcoder rewrites :path to the gRPC method path),
        # and forwards the bearer so the agent's AuthLayer still sees it.
        jwt = rest_jwt_of(self.render({}, self.JWT))
        rules = jwt["rules"]
        self.assertEqual([r["match"]["prefix"] for r in rules[:-1]], list(E.UNAUTHENTICATED_PREFIXES))
        self.assertTrue(all("requires" not in r for r in rules[:-1]))
        self.assertTrue(jwt["providers"]["agent"]["forward"], "the agent must still see the bearer")

    def test_adversarial_rest_catch_all_requires_a_token_no_open_prefix(self):
        # Fail-closed: the only exempt prefixes are the gRPC ones (never "/", "/v1", or a
        # REST prefix that would bypass the surface); every other path requires a token.
        # So an unmapped or un-rewritten /v1/... path falls through to the "/" rule → 401.
        jwt = rest_jwt_of(self.render({}, self.JWT))
        rules = jwt["rules"]
        exempt = [r["match"]["prefix"] for r in rules[:-1]]
        self.assertNotIn("/", exempt)
        self.assertFalse([p for p in exempt if p.startswith("/v1")],
                         "no REST-path exemption may bypass the transcoded surface")
        self.assertTrue(all(p.startswith(("/agent.", "/grpc.")) for p in exempt),
                        "exempt prefixes are gRPC method paths only")
        self.assertEqual(rules[-1], {"match": {"prefix": "/"}, "requires": {"provider_name": "agent"}})

    def test_positive_rest_reuses_the_gateway_cluster_no_duplicate(self):
        cfg = self.render({})
        route = rest_hcm(cfg)["route_config"]["virtual_hosts"][0]["routes"][0]
        self.assertEqual(route["route"]["cluster"], "agent_gateway")
        names = [c["name"] for c in cfg["static_resources"]["clusters"]]
        self.assertEqual(names.count("agent_gateway"), 1)

    def test_positive_container_paths_remap_the_descriptor(self):
        paths = {REST.descriptor: "/etc/envoy/agent_descriptor.pb"}
        tc = rest_transcoder_of(self.render({}, None, paths))
        self.assertEqual(tc["proto_descriptor"], "/etc/envoy/agent_descriptor.pb")

    def test_boundary_otlp_key_covers_the_rest_listener_too(self):
        # access log + tracer on each grpc-web listener AND the rest listener.
        text = json.dumps(self.render({"PORTAL_OTLP_AUTHORIZATION": "sekrit"}))
        self.assertEqual(text.count('"sekrit"'), 2 * (len(SPEC.listeners) + 1))


if __name__ == "__main__":
    unittest.main()
