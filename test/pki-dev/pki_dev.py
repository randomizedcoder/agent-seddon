#!/usr/bin/env python3
"""`nix run .#pki-dev` — an offline development PKI for the gRPC transport.

Drives `step certificate create` (smallstep step-cli; no `step-ca` daemon, no
network) to mint, under one output directory:

    ca/root.crt, ca/root.key                 the dev root CA (P-256)
    token-signer/cert.pem, key.pem           the agent-token signing key ([auth.token] signing_key)
                                             (security-hardening S5), SAN
                                             spiffe://agent.<deployment>/svc/token-signer
    <service>/cert.pem, key.pem              one leaf per --service: SANs localhost,
                                             127.0.0.1, ::1, <service>, and
                                             spiffe://agent.<deployment>/svc/<service>;
                                             EKU serverAuth + clientAuth; plus each
                                             --san (a host name or IP, e.g. the front
                                             door's public host)
    <service>/sans                           the SANs that leaf was minted with

The default output is $XDG_RUNTIME_DIR/agent-seddon/pki (tmpfs, per-user). Keys are
written unencrypted (`--no-password --insecure`) with mode 0600 in 0700 dirs — this
is a development CA; production brings its own (docs/grpc.md, "TLS").

Idempotent: an existing CA is reused and existing leaves are kept, so re-running
after adding a --service only mints the new one, and a leaf whose SANs changed (a new
--san) is re-minted under the same root, as is one that expires within
`--renew-within` (a daily timer keeps a long-running front door's leaf fresh).
`--force` regenerates everything.
`--verify` checks every leaf chains to the root (`step certificate verify`).

Design: docs/design/security-hardening/07-transport-tls-and-pki.md.
"""

from __future__ import annotations

import argparse
import ipaddress
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Mapping, Sequence

# A deployment / service name becomes a directory name and a DNS + SPIFFE SAN, so
# it is held to a DNS-label shape: no separators, dots, traversal or leading dash.
NAME_RE = re.compile(r"^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$")

# A --san host name: dot-separated DNS labels (letters, digits, '-'), at most 253
# bytes. No wildcard, scheme, port, space or comma, so one value is one SAN.
HOST_LABEL_RE = re.compile(r"^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$")
MAX_HOST = 253
SANS_FILE = "sans"
# --renew-within: whole hours, so the value passed to `step --expires-in` is one
# plain token. Capped below the leaf's own validity: a larger window re-mints on
# every run.
RENEW_RE = re.compile(r"^[1-9][0-9]{0,4}h$")

TOKEN_SIGNER = "token-signer"
DEFAULT_SERVICES = ("agent", "cli")
CA_VALIDITY = "87600h"  # 10 years: a dev root outlives the leaves it signs
LEAF_VALIDITY = "2160h"  # 90 days


class PkiError(Exception):
    """A usage or generation error, reported without a traceback."""


@dataclass(frozen=True)
class Step:
    """One `step` invocation plus the files it produces."""

    what: str
    args: tuple[str, ...]
    outputs: tuple[Path, ...]
    # A leaf's SAN record (`<leaf>/sans`), written after the leaf is minted.
    record: tuple[Path, str] | None = None


def validate_name(kind: str, name: str) -> str:
    if not NAME_RE.fullmatch(name):
        raise PkiError(
            f"{kind} {name!r} must be a DNS label: lowercase letters, digits and '-', "
            "1-63 chars, not starting or ending with '-'"
        )
    if name in ("ca", TOKEN_SIGNER) and kind == "service":
        raise PkiError(f"service name {name!r} is reserved")
    return name


def validate_san(value: str) -> str:
    """An extra SAN: an IP address or a host name, nothing else."""
    try:
        return str(ipaddress.ip_address(value))
    except ValueError:
        pass
    labels = value.split(".")
    if (
        not value
        or len(value) > MAX_HOST
        or not all(HOST_LABEL_RE.fullmatch(lab) for lab in labels)
        or labels[-1].isdigit()
    ):
        raise PkiError(
            f"--san {value!r} must be an IP address or a host name (dot-separated "
            "labels of letters, digits and '-', at most 253 chars)"
        )
    return value.lower()


def spiffe_id(deployment: str, name: str) -> str:
    return f"spiffe://agent.{deployment}/svc/{name}"


def leaf_sans(deployment: str, name: str) -> list[str]:
    return ["localhost", "127.0.0.1", "::1", name, spiffe_id(deployment, name)]


def default_out(env: Mapping[str, str]) -> Path:
    runtime = env.get("XDG_RUNTIME_DIR", "")
    if not runtime or not os.path.isabs(runtime):
        raise PkiError("XDG_RUNTIME_DIR is unset (or relative); pass --out DIR")
    return Path(runtime) / "agent-seddon" / "pki"


def ca_step(out: Path, deployment: str) -> Step:
    crt, key = out / "ca" / "root.crt", out / "ca" / "root.key"
    return Step(
        "root CA",
        (
            "certificate", "create", f"agent-seddon {deployment} dev root CA",
            str(crt), str(key),
            "--profile", "root-ca", "--kty", "EC", "--curve", "P-256",
            "--not-after", CA_VALIDITY, "--no-password", "--insecure",
        ),
        (crt, key),
    )


def leaf_step(out: Path, deployment: str, name: str, sans: Sequence[str]) -> Step:
    crt, key = out / name / "cert.pem", out / name / "key.pem"
    args = [
        "certificate", "create", name, str(crt), str(key),
        "--profile", "leaf",
        "--ca", str(out / "ca" / "root.crt"), "--ca-key", str(out / "ca" / "root.key"),
        "--kty", "EC", "--curve", "P-256",
        "--not-after", LEAF_VALIDITY, "--no-password", "--insecure",
    ]
    for san in sans:
        args += ["--san", san]
    return Step(f"leaf {name}", tuple(args), (crt, key), (out / name / SANS_FILE, sans_text(sans)))


def sans_text(sans: Sequence[str]) -> str:
    return "".join(f"{s}\n" for s in sans)


def plan(
    out: Path,
    deployment: str,
    services: Sequence[str],
    force: bool,
    exists: Callable[[Path], bool],
    extra_sans: Sequence[str] = (),
    recorded: Callable[[Path], str | None] = lambda _: None,
    due: Callable[[Path], bool] = lambda _: False,
) -> list[Step]:
    """The `step` calls still needed. `force` ⇒ all of them; else skip what exists.

    A missing CA forces every leaf too: a leaf signed by a previous, deleted root
    would no longer verify. A leaf whose recorded SANs differ from the ones it
    would get now is re-minted under the same root; a leaf with no record was
    minted before `--san` existed, with the default SANs. `due(cert)` says a leaf
    expires soon (`--renew-within`); the root is never renewed this way.
    """
    validate_name("deployment", deployment)
    names = [validate_name("service", s) for s in services]
    if len(set(names)) != len(names):
        raise PkiError("duplicate --service")
    extra = [validate_san(s) for s in extra_sans]
    ca = ca_step(out, deployment)
    new_ca = force or not all(exists(p) for p in ca.outputs)
    steps = [ca] if new_ca else []
    leaves = [leaf_step(out, deployment, TOKEN_SIGNER, [spiffe_id(deployment, TOKEN_SIGNER)])]
    for n in names:
        sans = leaf_sans(deployment, n)
        leaves.append(leaf_step(out, deployment, n, sans + [s for s in extra if s not in sans]))
    for leaf in leaves:
        missing = not all(exists(p) for p in leaf.outputs)
        if new_ca or missing or san_changed(leaf, deployment, recorded) or due(leaf.outputs[0]):
            steps.append(leaf)
    return steps


def san_changed(leaf: Step, deployment: str, recorded: Callable[[Path], str | None]) -> bool:
    if leaf.record is None:
        return False
    path, want = leaf.record
    have = recorded(path)
    if have is None:
        name = path.parent.name
        default = [spiffe_id(deployment, name)] if name == TOKEN_SIGNER else leaf_sans(deployment, name)
        have = sans_text(default)
    return have != want


def read_record(path: Path) -> str | None:
    try:
        return path.read_text()
    except FileNotFoundError:
        return None


def validate_renew(value: str) -> str:
    hours = int(value[:-1]) if RENEW_RE.fullmatch(value) else 0
    if not 0 < hours < int(LEAF_VALIDITY[:-1]):
        raise PkiError(
            f"--renew-within {value!r} must be whole hours like 720h, under the "
            f"leaf validity ({LEAF_VALIDITY})"
        )
    return value


def renewal_due(step_bin: str, run: Runner, within: str) -> Callable[[Path], bool]:
    """`step certificate needs-renewal`: 0 renew, 1 fine; anything else (an
    unreadable or corrupt leaf) is treated as due, so it is re-minted."""
    return lambda cert: run([step_bin, "certificate", "needs-renewal", str(cert), "--expires-in", within]) != 1


def config_snippet(out: Path, server: str, client: str) -> str:
    return (
        "# config/agent.toml — mTLS with this dev PKI (dial seams as https://…)\n"
        "[grpc.tls]\n"
        f'cert = "{out / server / "cert.pem"}"\n'
        f'key = "{out / server / "key.pem"}"\n'
        f'client_ca = "{out / "ca" / "root.crt"}"\n'
        "[grpc.tls.client]\n"
        f'ca = "{out / "ca" / "root.crt"}"\n'
        f'cert = "{out / client / "cert.pem"}"\n'
        f'key = "{out / client / "key.pem"}"\n'
    )


Runner = Callable[[Sequence[str]], int]


def subprocess_runner(argv: Sequence[str]) -> int:
    return subprocess.run(list(argv), check=False).returncode


def generate(out: Path, steps: Sequence[Step], step_bin: str, run: Runner) -> None:
    for s in steps:
        for p in s.outputs:
            p.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            if p.exists():
                p.unlink()  # step refuses to overwrite without a prompt
        if run([step_bin, *s.args]) != 0:
            raise PkiError(f"`step` failed creating the {s.what}")
        for p in s.outputs:
            if not p.exists():
                raise PkiError(f"`step` reported success but {p} is missing")
            p.chmod(0o600)
        if s.record is not None:
            path, text = s.record
            path.write_text(text)
            path.chmod(0o600)
    out.chmod(0o700)


def leaf_dirs(out: Path) -> list[Path]:
    return sorted(
        d for d in out.iterdir() if d.is_dir() and d.name != "ca" and (d / "cert.pem").exists()
    )


def verify(out: Path, step_bin: str, run: Runner) -> list[str]:
    """Every leaf must chain to the root. Returns the failures (empty ⇒ all good)."""
    root = out / "ca" / "root.crt"
    if not root.exists():
        return [f"no root CA at {root}"]
    leaves = leaf_dirs(out)
    if not leaves:
        return [f"no leaf certificates under {out}"]
    failures = []
    for d in leaves:
        # Against this root only (no system roots): chain + validity window.
        rc = run([step_bin, "certificate", "verify", str(d / "cert.pem"), "--roots", str(root)])
        if rc != 0:
            failures.append(f"{d.name}: does not verify against {root}")
    return failures


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    p = argparse.ArgumentParser(prog="pki-dev", description=__doc__.split("\n\n")[0])
    p.add_argument("--out", type=Path, help="output dir (default $XDG_RUNTIME_DIR/agent-seddon/pki)")
    p.add_argument("--deployment", default="dev", help="SPIFFE trust-domain label (default dev)")
    p.add_argument(
        "--service", action="append", dest="services",
        help=f"leaf to mint (repeatable; default {' '.join(DEFAULT_SERVICES)})",
    )
    p.add_argument(
        "--san", action="append", dest="sans", default=[],
        help="extra SAN on every --service leaf: a host name or IP (repeatable)",
    )
    p.add_argument(
        "--renew-within", metavar="HOURS",
        help="re-mint a leaf that expires within this many hours, e.g. 720h (the root is kept)",
    )
    p.add_argument("--force", action="store_true", help="regenerate the CA and every leaf")
    p.add_argument("--verify", action="store_true", help="only verify an existing PKI")
    p.add_argument("--step", default="step", help="step-cli binary (default: on PATH)")
    return p.parse_args(list(argv))


def main(
    argv: Sequence[str],
    env: Mapping[str, str] = os.environ,
    run: Runner = subprocess_runner,
) -> int:
    args = parse_args(argv)
    try:
        out = (args.out or default_out(env)).absolute()
        if args.verify:
            failures = verify(out, args.step, run)
            for f in failures:
                print(f"pki-dev: FAIL {f}", file=sys.stderr)
            if not failures:
                print(f"pki-dev: every leaf under {out} verifies against the root")
            return 1 if failures else 0
        services = args.services or list(DEFAULT_SERVICES)
        # `--force` never deletes the directory (it may be one the operator
        # pointed --out at): `generate` replaces only the files it writes.
        due = (
            renewal_due(args.step, run, validate_renew(args.renew_within))
            if args.renew_within is not None
            else (lambda _: False)
        )
        steps = plan(out, args.deployment, services, args.force, Path.exists, args.sans, read_record, due)
        out.mkdir(mode=0o700, parents=True, exist_ok=True)
        generate(out, steps, args.step, run)
        made = ", ".join(s.what for s in steps) or "nothing (all present; --force regenerates)"
        print(f"pki-dev: {out}: minted {made}")
        names = [validate_name("service", s) for s in services]
        client = names[1] if len(names) > 1 else names[0]
        print(config_snippet(out, names[0], client))
        return 0
    except PkiError as e:
        print(f"pki-dev: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
