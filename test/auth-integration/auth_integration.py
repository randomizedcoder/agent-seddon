"""auth-integration: the sign-in stack over real infrastructure (security-hardening S15b).

`auth-e2e` (S15a) proves the sign-in chain inside the nix sandbox with offline
certificates and file-backed sessions. This harness runs the same two real agents
against the infrastructure a deployment actually uses, which the sandbox cannot
host:

- **step-ca tier** (always; native binaries, no container): a real `step-ca`
  daemon issues every certificate over its provisioner API. The agents serve and
  dial with those certificates, the fleet certificate is renewed through the
  daemon (`step ca renew`), and a certificate from a different CA carrying the
  same SPIFFE name is refused.
- **Postgres tier**: agent A keeps sign-in sessions in a throwaway Postgres
  (`[auth.token] session_store = "postgres"`, schema applied by the agent). Rows
  land per tenant, no refresh handle is stored in clear, and sessions and
  revocations survive a restart of the agent.
- **ClickHouse tier**: agent A writes `agent_auth_events` into a throwaway
  ClickHouse with the shipped schema and credentials. The expected events land,
  `agent_reader` sees only the tenant it is scoped to, and no row carries token
  material.

The Postgres and ClickHouse tiers need a container runtime ($CONTAINER_RUNTIME,
default docker) and are skipped with a notice without one; the step-ca tier still
runs. Containers use their own names and ports (nix/versions.nix) and are removed
on exit, so the long-lived dev servers are never touched.

Exit codes (the shared contract): 0 clean (or tiers skipped), 1 harness failure,
2 contract failure.
"""

from __future__ import annotations

import argparse
import json
import os
import secrets
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.parse
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Mapping, Sequence

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "auth-e2e"))
sys.path.insert(0, str(HERE.parent / "clickhouse"))

import auth_e2e as e2e  # noqa: E402
import ch_creds  # noqa: E402
import rls_harness  # noqa: E402

from auth_e2e import (  # noqa: E402
    EXIT_CONTRACT,
    EXIT_HARNESS,
    EXIT_OK,
    TENANT_A,
    TENANT_B,
    Client,
    ContractError,
    HarnessError,
    expect,
)

PG_USER = "agent"
PG_DB = "agent_auth"
PG_DSN_ENV = "AUTH_IT_PG_DSN"
SERVICES = ("agent", "memory", "fleet", "cli")
SIGNER = "token-signer"
PROVISIONER = "admin"
WAIT_SECS = 30.0
AUDIT_WAIT_SECS = 30.0

Run = Callable[..., subprocess.CompletedProcess]


# ---------------------------------------------------------------- step-ca


def ca_init_argv(step: str, port: int, password_file: Path) -> list[str]:
    return [
        step, "ca", "init",
        "--deployment-type", "standalone",
        "--name", "agent-seddon auth-integration",
        "--dns", "localhost", "--dns", "127.0.0.1",
        "--address", f"127.0.0.1:{port}",
        "--provisioner", PROVISIONER,
        "--password-file", str(password_file),
        "--provisioner-password-file", str(password_file),
    ]


def ca_certificate_argv(
    step: str, subject: str, crt: Path, key: Path, sans: Sequence[str],
    ca_url: str, root: Path, password_file: Path,
) -> list[str]:
    argv = [
        step, "ca", "certificate",
        "--provisioner", PROVISIONER,
        "--provisioner-password-file", str(password_file),
        "--ca-url", ca_url, "--root", str(root),
        "--kty", "EC", "--curve", "P-256", "--force",
    ]
    for san in sans:
        argv += ["--san", san]
    return argv + [subject, str(crt), str(key)]


def ca_renew_argv(step: str, crt: Path, key: Path, ca_url: str, root: Path) -> list[str]:
    # `step ca renew` rejects flags after its positionals ("too many positional
    # arguments"), so every flag goes first.
    return [step, "ca", "renew", "--force", "--ca-url", ca_url, "--root", str(root), str(crt), str(key)]


@dataclass(frozen=True)
class CertInfo:
    serial: str
    not_after: str
    uris: tuple[str, ...]


def parse_inspect(text: str) -> CertInfo:
    """`step certificate inspect --format json` → the fields the steps compare."""
    try:
        d = json.loads(text)
        san = (d.get("extensions") or {}).get("subject_alt_name") or {}
        return CertInfo(
            serial=str(d["serial_number"]),
            not_after=str(d["validity"]["end"]),
            uris=tuple(san.get("uniform_resource_identifiers") or ()),
        )
    except (ValueError, KeyError, TypeError, AttributeError) as e:
        raise HarnessError(f"unreadable certificate inspection: {e}") from e


class StepCa:
    """A real `step-ca` daemon under a private STEPPATH (never ~/.step)."""

    def __init__(self, step: str, step_ca: str, root_dir: Path, port: int, run: Run = subprocess.run):
        self.step, self.step_ca, self.port, self.run = step, step_ca, port, run
        self.steppath = root_dir / "steppath"
        self.password_file = root_dir / "password"
        self.log = root_dir / "step-ca.log"
        self.proc: subprocess.Popen | None = None

    @property
    def url(self) -> str:
        return f"https://127.0.0.1:{self.port}"

    @property
    def root(self) -> Path:
        return self.steppath / "certs" / "root_ca.crt"

    @property
    def env(self) -> dict:
        return {**os.environ, "STEPPATH": str(self.steppath)}

    def _step(self, argv: Sequence[str], what: str) -> str:
        p = self.run(list(argv), capture_output=True, text=True, env=self.env)
        if p.returncode != 0:
            raise HarnessError(f"{what} failed:\n{p.stdout}{p.stderr}")
        return p.stdout

    def init(self) -> None:
        self.password_file.parent.mkdir(parents=True, exist_ok=True)
        self.password_file.write_text(secrets.token_urlsafe(24))
        self.password_file.chmod(0o600)
        self._step(ca_init_argv(self.step, self.port, self.password_file), "step ca init")

    def start(self) -> None:
        with self.log.open("w") as out:
            self.proc = subprocess.Popen(
                [self.step_ca, str(self.steppath / "config" / "ca.json"), "--password-file", str(self.password_file)],
                stdout=out, stderr=subprocess.STDOUT, env=self.env,
            )
        deadline = time.monotonic() + WAIT_SECS
        while time.monotonic() < deadline:
            if self.proc.poll() is not None:
                raise HarnessError(f"step-ca exited during start-up:\n{self.log.read_text(errors='replace')[-2000:]}")
            p = self.run([self.step, "ca", "health", "--ca-url", self.url, "--root", str(self.root)],
                         capture_output=True, text=True, env=self.env)
            if p.returncode == 0:
                return
            time.sleep(0.2)
        raise HarnessError("step-ca never reported healthy")

    def issue(self, subject: str, crt: Path, key: Path, sans: Sequence[str]) -> None:
        crt.parent.mkdir(parents=True, exist_ok=True)
        self._step(ca_certificate_argv(self.step, subject, crt, key, sans, self.url, self.root, self.password_file),
                   f"step ca certificate {subject}")

    def renew(self, crt: Path, key: Path) -> None:
        self._step(ca_renew_argv(self.step, crt, key, self.url, self.root), f"step ca renew {crt}")

    def inspect(self, crt: Path) -> CertInfo:
        return parse_inspect(self._step([self.step, "certificate", "inspect", "--format", "json", str(crt)],
                                        f"step certificate inspect {crt}"))

    def stop(self) -> None:
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()


def build_pki(ca: StepCa, pki: Path) -> None:
    """Lay the daemon's certificates out the way `pki-dev` does, so the S15a
    config renderer and clients take them unchanged."""
    for svc in (*SERVICES, SIGNER):
        ca.issue(svc, pki / svc / "cert.pem", pki / svc / "key.pem", (e2e.spiffe(svc), "localhost"))
    (pki / "ca").mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ca.root, pki / "ca" / "root.crt")


# ---------------------------------------------------------------- Postgres


def pg_dsn(password: str, port: int) -> str:
    return f"postgres://{PG_USER}:{urllib.parse.quote(password, safe='')}@127.0.0.1:{port}/{PG_DB}"


class Postgres:
    """A throwaway, volume-less Postgres. The password rides the environment,
    never argv."""

    def __init__(self, runtime: str, name: str, image: str, port: int, run: Run = subprocess.run):
        self.runtime, self.name, self.image, self.port, self.run = runtime, name, image, port, run
        self.password = secrets.token_hex(16)

    @property
    def dsn(self) -> str:
        return pg_dsn(self.password, self.port)

    def _rt(self, *args: str, stdin: str | None = None) -> subprocess.CompletedProcess:
        return self.run([self.runtime, *args], input=stdin, capture_output=True, text=True,
                        env={**os.environ, "POSTGRES_PASSWORD": self.password})

    def start(self) -> None:
        self.remove()
        p = self._rt("run", "-d", "--name", self.name, "-p", f"127.0.0.1:{self.port}:5432",
                     "-e", "POSTGRES_PASSWORD", "-e", f"POSTGRES_USER={PG_USER}", "-e", f"POSTGRES_DB={PG_DB}",
                     self.image)
        if p.returncode:
            raise HarnessError(f"postgres container did not start: {p.stderr.strip()}")
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            # pg_isready is up before the init scripts finish; a real query is the barrier.
            if self._rt("exec", self.name, "psql", "-U", PG_USER, "-d", PG_DB, "-h", "127.0.0.1",
                        "-Atc", "SELECT 1").returncode == 0:
                return
            time.sleep(1)
        raise HarnessError("postgres never answered a query")

    def sql(self, query: str) -> str:
        p = self._rt("exec", "-i", self.name, "psql", "-U", PG_USER, "-d", PG_DB,
                     "-v", "ON_ERROR_STOP=1", "-At", stdin=query)
        if p.returncode:
            raise HarnessError(f"postgres query failed: {p.stderr.strip()[:300]}")
        return p.stdout

    def remove(self) -> None:
        self._rt("rm", "-f", self.name)


# ---------------------------------------------------------------- config


def session_lines(pg: bool, state: Path) -> tuple[str, ...]:
    if pg:
        return ('session_store = "postgres"',)
    return ('session_store = "file"', f"session_path = {e2e.toml_str(state / 'auth-sessions')}")


def extra_sections(*, pg: bool, ch_native: int | None, writer_password_file: Path | None) -> list[str]:
    lines: list[str] = []
    if pg:
        lines += [
            "[config_store]",
            f'dsn_ref = "env:{PG_DSN_ENV}"',
            "migrate_on_start = true",
            "",
        ]
    if ch_native is not None:
        if writer_password_file is None:
            raise ValueError("the ClickHouse tier needs the writer password file")
        lines += [
            "[telemetry]",
            "enabled = true",
            f'clickhouse_url = "127.0.0.1:{ch_native}"',
            'user = "agent_writer"',
            f"password_file = {e2e.toml_str(writer_password_file)}",
            "flush_interval_ms = 200",
            "",
        ]
    return lines


# ---------------------------------------------------------------- the contract


ChQuery = Callable[[str, str], "rls_harness.Reply"]  # (role, sql) → reply


@dataclass
class ItCtx(e2e.Ctx):
    ca: StepCa | None = None
    foreign: Client | None = None  # the fleet SAN, minted by a different CA
    pg: Callable[[str], str] | None = None
    ch: ChQuery | None = None
    restart_a: Callable[[], None] | None = None
    audit_wait: float = AUDIT_WAIT_SECS
    svc_tokens: list = field(default_factory=list)


EXCHANGE = "agent.v1.AuthService/Exchange"
WHOAMI = "agent.v1.AuthService/WhoAmI"
REFRESH = "agent.v1.AuthService/Refresh"
LOGOUT = "agent.v1.AuthService/Logout"


def _svc_token(ctx: ItCtx, what: str) -> str:
    reply = expect(ctx.fleet.call(ctx.lay.addr_a, EXCHANGE, {"useClientCert": True}), "OK", what)
    token = reply.body.get("accessToken", "")
    if not token:
        raise ContractError(f"{what}: no access token in {reply.body}")
    ctx.svc_tokens.append(token)
    return token


def step_renewal_keeps_identity(ctx: ItCtx) -> None:
    """Renew the fleet certificate through the daemon: a new certificate, the same
    service identity, tokens in flight across the rotation, and the pre-renewal
    token still useless off a bound service's connection."""
    crt, key = ctx.lay.leaf("fleet")
    before = ctx.ca.inspect(crt)
    old_token = _svc_token(ctx, "service Exchange before renewal")
    ctx.ca.renew(crt, key)
    after = ctx.ca.inspect(crt)
    if after.serial == before.serial:
        raise ContractError(f"step ca renew kept serial {before.serial}")
    if after.not_after < before.not_after:
        raise ContractError(f"renewal shortened validity: {before.not_after} → {after.not_after}")
    if e2e.spiffe("fleet") not in after.uris:
        raise ContractError(f"renewed certificate lost its SPIFFE name: {after.uris}")
    new_token = _svc_token(ctx, "service Exchange with the renewed certificate")
    who = expect(ctx.fleet.call(ctx.lay.addr_a, WHOAMI, bearer=new_token), "OK", "WhoAmI after renewal")
    if who.body.get("tenant") != TENANT_A or not str(who.body.get("subject", "")).startswith("svc:"):
        raise ContractError(f"renewed certificate mapped to {who.body}, want svc: in {TENANT_A}")
    # `cnf` names the old certificate's thumbprint. The renewed certificate is
    # still a bound service, and a bound service may carry a token it did not
    # present (S9 relaying), so a token minted before renewal keeps working over
    # it: rotation does not strand work in flight. Off a bound service's
    # connection the old token is refused, as any certificate-bound token is.
    expect(ctx.fleet.call(ctx.lay.addr_a, WHOAMI, bearer=old_token), "OK",
           "a pre-renewal token over the renewed certificate (rotation keeps work in flight)")
    expect(ctx.user.call(ctx.lay.addr_a, WHOAMI, bearer=old_token), "Unauthenticated",
           "the pre-renewal token over an unbound certificate")
    # With no certificate at all the mTLS listener refuses the handshake (DIAL);
    # a listener that let it through must still refuse the token.
    bare = ctx.bare.call(ctx.lay.addr_a, WHOAMI, bearer=old_token)
    if bare.code not in ("DIAL", "Unauthenticated"):
        raise ContractError(f"the pre-renewal token with no client certificate: got {bare.code}")


def step_foreign_ca_refused(ctx: ItCtx) -> None:
    """A certificate for the same SPIFFE name from another CA gets nowhere."""
    for addr in (ctx.lay.addr_a, ctx.lay.addr_b):
        reply = ctx.foreign.call(addr, "grpc.health.v1.Health/Check")
        if reply.code == "OK":
            raise ContractError(f"{addr} accepted a client certificate from a foreign CA")
    reply = ctx.foreign.call(ctx.lay.addr_a, EXCHANGE, {"useClientCert": True})
    if reply.code == "OK":
        raise ContractError("a foreign-CA certificate with the fleet SAN got a service token")


def parse_counts(text: str) -> dict[str, int]:
    """psql `-At` rows `tenant|count` → {tenant: count}."""
    out: dict[str, int] = {}
    for line in text.splitlines():
        if not line.strip():
            continue
        tenant, sep, n = line.rpartition("|")
        if not sep or not n.isdigit():
            raise HarnessError(f"unexpected psql row {line!r}")
        out[tenant] = int(n)
    return out


def step_sessions_in_postgres(ctx: ItCtx) -> None:
    counts = parse_counts(ctx.pg(
        "SELECT tenant, count(*) FROM cards WHERE collection = 'auth_sessions' GROUP BY tenant ORDER BY tenant;"))
    for tenant in (TENANT_A, TENANT_B):
        if counts.get(tenant, 0) < 1:
            raise ContractError(f"no auth_sessions rows for {tenant} in Postgres: {counts}")
    # hex, not base64: psql wraps base64 output, hex stays one line per row.
    blobs = [bytes.fromhex(row) for row in ctx.pg(
        "SELECT encode(blob, 'hex') FROM cards WHERE collection = 'auth_sessions';").split()]
    for who, tok in ctx.tokens.items():
        for kind, value in tok.items():
            if value and any(value.encode() in blob for blob in blobs):
                raise ContractError(f"{who}'s {kind} secret is stored in clear in Postgres")


def step_sessions_survive_restart(ctx: ItCtx) -> None:
    bob = ctx.tokens["bob"]
    ctx.restart_a()
    reply = expect(ctx.user.call(ctx.lay.addr_a, REFRESH, {"refreshHandle": bob["refresh"]}), "OK",
                   "Refresh with a handle issued before the restart")
    rotated = reply.body.get("refreshHandle", "")
    if not rotated or rotated == bob["refresh"]:
        raise ContractError(f"Refresh after restart did not rotate the handle: {reply.body}")
    expect(ctx.user.call(ctx.lay.addr_a, REFRESH, {"refreshHandle": bob["refresh"]}), "Unauthenticated",
           "a replayed pre-restart handle")
    ctx.restart_a()
    expect(ctx.user.call(ctx.lay.addr_a, REFRESH, {"refreshHandle": rotated}), "Unauthenticated",
           "a session revoked by replay, after another restart")
    alice = ctx.tokens["alice"]
    expect(ctx.user.call(ctx.lay.addr_a, LOGOUT, bearer=alice["access"]), "OK", "Logout for alice")
    expect(ctx.user.call(ctx.lay.addr_a, REFRESH, {"refreshHandle": alice["refresh"]}), "Unauthenticated",
           "Refresh after Logout")


# (tenant, event) pairs the run above must leave behind. `verify_fail` rows carry
# no tenant: nothing was proven.
EXPECTED_EVENTS = frozenset({
    (TENANT_A, "login"),
    (TENANT_B, "login"),
    (TENANT_B, "refresh"),
    (TENANT_B, "revoke"),
    (TENANT_A, "logout"),
    ("", "verify_fail"),
})


def parse_pairs(body: str) -> set[tuple[str, str]]:
    out = set()
    for line in body.splitlines():
        if line.strip():
            parts = line.split("\t")
            if len(parts) != 2:
                raise HarnessError(f"unexpected ClickHouse row {line!r}")
            out.add((parts[0], parts[1]))
    return out


def ch_ok(ctx: ItCtx, role: str, sql: str) -> str:
    reply = ctx.ch(role, sql)
    if not reply.ok:
        raise ContractError(f"ClickHouse as {role} refused {sql!r}: {reply.body.strip()[:300]}")
    return reply.body


def step_audit_rows_land(ctx: ItCtx) -> None:
    expect(ctx.user.call(ctx.lay.addr_a, WHOAMI, bearer="not-a-token"), "Unauthenticated", "a garbage bearer")
    sql = "SELECT DISTINCT user, event FROM agent.agent_auth_events FORMAT TSV"
    deadline = time.monotonic() + ctx.audit_wait
    while True:
        got = parse_pairs(ch_ok(ctx, "writer", sql))
        missing = EXPECTED_EVENTS - got
        if not missing:
            return
        if time.monotonic() >= deadline:
            raise ContractError(f"audit rows missing after {ctx.audit_wait:.0f}s: {sorted(missing)}; got {sorted(got)}")
        time.sleep(0.5)


def step_audit_rows_tenant_scoped(ctx: ItCtx) -> None:
    for tenant in (TENANT_A, TENANT_B):
        body = ch_ok(ctx, "reader", "SELECT DISTINCT user FROM agent.agent_auth_events "
                     f"SETTINGS SQL_tenant_id = '{tenant}' FORMAT TSV")
        seen = {line for line in body.splitlines() if line}
        if seen != {tenant}:
            raise ContractError(f"agent_reader scoped to {tenant} saw tenants {sorted(seen)}")
    reply = ctx.ch("reader", "SELECT count() FROM agent.agent_auth_events FORMAT TSV")
    if reply.ok and reply.body.strip() != "0":
        raise ContractError(f"agent_reader with no tenant read {reply.body.strip()} audit rows")


def step_audit_rows_carry_no_secrets(ctx: ItCtx) -> None:
    body = ch_ok(ctx, "writer", "SELECT * FROM agent.agent_auth_events FORMAT TSV")
    secrets_seen = [t for tok in ctx.tokens.values() for t in tok.values() if t]
    secrets_seen += ctx.svc_tokens
    for value in secrets_seen:
        if value in body:
            raise ContractError("an audit row carries a token or refresh handle")
    if "eyJ" in body or "not-a-token" in body:
        raise ContractError("an audit row carries bearer material")


Step = tuple[str, Callable[[ItCtx], None]]

STEPCA_STEPS: tuple[Step, ...] = (
    ("health over step-ca certificates", e2e.step_health_is_exempt),
    ("Exchange mints agent tokens", e2e.step_exchange_mints_agent_tokens),
    ("chain forwards the tenant across a grpc seam", e2e.step_chain_forwards_tenant),
    ("mTLS service identity", e2e.step_mtls_service_identity),
    ("renewal through the daemon keeps the identity", step_renewal_keeps_identity),
    ("a foreign CA is refused", step_foreign_ca_refused),
)
PG_STEPS: tuple[Step, ...] = (
    ("sessions persist in Postgres, secrets hashed", step_sessions_in_postgres),
    ("sessions and revocations survive a restart", step_sessions_survive_restart),
)
FILE_SESSION_STEPS: tuple[Step, ...] = (
    ("refresh rotation and logout", e2e.step_refresh_and_logout),
)
CH_STEPS: tuple[Step, ...] = (
    ("audit rows land in ClickHouse", step_audit_rows_land),
    ("audit rows are tenant-scoped for agent_reader", step_audit_rows_tenant_scoped),
    ("audit rows carry no secrets", step_audit_rows_carry_no_secrets),
)


def plan(pg: bool, ch: bool) -> tuple[Step, ...]:
    """The step list for the tiers that are up. Without Postgres the session
    steps run over the file store (the S15a refresh step); the audit steps expect
    the Postgres step's events, so ClickHouse without Postgres is not a plan."""
    if ch and not pg:
        raise ValueError("the ClickHouse tier needs the Postgres tier's session events")
    return STEPCA_STEPS + (PG_STEPS if pg else FILE_SESSION_STEPS) + (CH_STEPS if ch else ())


# ---------------------------------------------------------------- main


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    p = argparse.ArgumentParser(prog="auth-integration", description=__doc__.split("\n\n")[0])
    p.add_argument("--agent", default="agent")
    p.add_argument("--grpcurl", default="grpcurl")
    p.add_argument("--step", default="step")
    p.add_argument("--step-ca", default="step-ca")
    p.add_argument("--pki-dev", default="pki-dev", help="the pki-dev command (split on spaces)")
    p.add_argument("--ca-port", type=int, required=True)
    p.add_argument("--runtime", default=os.environ.get("CONTAINER_RUNTIME", "docker"))
    p.add_argument("--pg-image", required=True)
    p.add_argument("--pg-name", required=True)
    p.add_argument("--pg-port", type=int, required=True)
    p.add_argument("--ch-image", required=True)
    p.add_argument("--ch-name", required=True)
    p.add_argument("--ch-http-port", type=int, required=True)
    p.add_argument("--ch-native-port", type=int, required=True)
    p.add_argument("--schema", type=Path, required=True)
    p.add_argument("--config-xml", type=Path, required=True)
    p.add_argument("--users-xml", type=Path, required=True)
    p.add_argument("--keep", action="store_true", help="keep the work directory")
    return p.parse_args(argv)


def runtime_up(runtime: str) -> bool:
    try:
        return subprocess.run([runtime, "info"], capture_output=True).returncode == 0
    except OSError:
        return False


def mint_foreign(pki_dev: Sequence[str], out: Path) -> None:
    p = subprocess.run([*pki_dev, "--out", str(out), "--deployment", e2e.DEPLOYMENT, "--service", "fleet"],
                       capture_output=True, text=True)
    if p.returncode != 0:
        raise HarnessError(f"pki-dev failed:\n{p.stdout}{p.stderr}")


def main(argv: Sequence[str]) -> int:
    args = parse_args(argv)
    for tool in (args.agent, args.grpcurl, args.step, args.step_ca):
        if shutil.which(tool) is None:
            print(f"FAIL(harness): {tool} not found", file=sys.stderr)
            return EXIT_HARNESS
    containers = runtime_up(args.runtime)
    if not containers:
        print(f"auth-integration: SKIP Postgres + ClickHouse tiers — container runtime ({args.runtime}) "
              "not reachable; the step-ca tier still runs.")
    work = Path(tempfile.mkdtemp(prefix="auth-integration-"))
    issuer = e2e.Issuer()
    ca = StepCa(args.step, args.step_ca, work / "ca", args.ca_port)
    pg = Postgres(args.runtime, args.pg_name, "docker.io/" + args.pg_image, args.pg_port) if containers else None
    ch = (rls_harness.Container(args.runtime, args.ch_name, "docker.io/" + args.ch_image,
                                args.ch_http_port, args.ch_native_port) if containers else None)
    servers: dict[str, e2e.Server] = {}
    try:
        issuer.start()
        lay = e2e.Layout(work, work / "pki", issuer.url, e2e.free_port(), e2e.free_port())
        print("==> auth-integration: step-ca daemon")
        ca.init()
        ca.start()
        build_pki(ca, lay.pki)
        mint_foreign(args.pki_dev.split(), work / "foreign")

        ch_pw: dict[str, str] = {}
        creds = work / "ch-creds"
        if pg is not None:
            print("==> auth-integration: Postgres")
            pg.start()
        if ch is not None:
            print("==> auth-integration: ClickHouse")
            ch_creds.ensure(creds)
            ch_pw = {r: ch_creds.read_password(creds, r) for r in ch_creds.ROLES}
            ch.start(args.config_xml, args.users_xml, ch_creds.server_xml_path(creds))
            ch.admin_sql(args.schema.read_text(), ch_pw["admin"])
            ch.admin_sql(ch_creds.alter_sql(ch_pw), ch_pw["admin"])

        cli_cert, cli_key = lay.leaf("cli")
        fleet_cert, fleet_key = lay.leaf("fleet")
        query = rls_harness.http_query(f"http://127.0.0.1:{args.ch_http_port}")
        users = {"writer": "agent_writer", "reader": "agent_reader"}
        ctx = ItCtx(
            lay=lay,
            issuer=issuer,
            user=Client(args.grpcurl, lay.ca, cli_cert, cli_key),
            fleet=Client(args.grpcurl, lay.ca, fleet_cert, fleet_key),
            bare=Client(args.grpcurl, lay.ca),
            ca=ca,
            foreign=Client(args.grpcurl, lay.ca, work / "foreign" / "fleet" / "cert.pem",
                           work / "foreign" / "fleet" / "key.pem"),
            pg=pg.sql if pg else None,
            ch=(lambda role, sql: query(users[role], ch_pw[role], sql)) if ch else None,
        )
        config_a = e2e.render_config(
            lay, "a",
            sessions=session_lines(pg is not None, work / "a"),
            extra=extra_sections(
                pg=pg is not None,
                ch_native=args.ch_native_port if ch else None,
                writer_password_file=ch_creds.password_path(creds, "writer") if ch else None,
            ),
        )
        env_a = {PG_DSN_ENV: pg.dsn} if pg else {}

        def start_a() -> None:
            servers["a"] = e2e.start_server(args.agent, lay, "a", ["--serve-all"], ctx.user, lay.addr_a,
                                            config=config_a, env=env_a)

        def restart_a() -> None:
            servers.pop("a").stop()
            start_a()

        ctx.restart_a = restart_a
        servers["b"] = e2e.start_server(args.agent, lay, "b", ["--serve-memory"], ctx.user, lay.addr_b)
        start_a()
        code = e2e.run_steps(ctx, plan(pg is not None, ch is not None))
        if code != EXIT_OK:
            for srv in servers.values():
                print(f"--- server {srv.name} log tail\n{srv.tail()}", file=sys.stderr)
        elif not containers:
            print("PASS: auth-integration — step-ca tier (Postgres + ClickHouse skipped).")
        else:
            print("PASS: auth-integration — step-ca, Postgres sessions and ClickHouse audit hold.")
        return code
    except (HarnessError, ch_creds.CredsError, rls_harness.HarnessError) as e:
        print(f"FAIL(harness): {e}", file=sys.stderr)
        for srv in servers.values():
            print(f"--- server {srv.name} log tail\n{srv.tail()}", file=sys.stderr)
        return EXIT_HARNESS
    finally:
        for srv in servers.values():
            srv.stop()
        ca.stop()
        issuer.stop()
        for c in (pg, ch):
            if c is not None:
                c.remove()
        if args.keep:
            print(f"work dir kept: {work}", file=sys.stderr)
        else:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
