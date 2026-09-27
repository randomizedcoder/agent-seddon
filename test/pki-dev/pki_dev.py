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
                                             EKU serverAuth + clientAuth

The default output is $XDG_RUNTIME_DIR/agent-seddon/pki (tmpfs, per-user). Keys are
written unencrypted (`--no-password --insecure`) with mode 0600 in 0700 dirs — this
is a development CA; production brings its own (docs/grpc.md, "TLS").

Idempotent: an existing CA is reused and existing leaves are kept, so re-running
after adding a --service only mints the new one. `--force` regenerates everything.
`--verify` checks every leaf chains to the root (`step certificate verify`).

Design: docs/design/security-hardening/07-transport-tls-and-pki.md.
"""

from __future__ import annotations

import argparse
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


def validate_name(kind: str, name: str) -> str:
    if not NAME_RE.fullmatch(name):
        raise PkiError(
            f"{kind} {name!r} must be a DNS label: lowercase letters, digits and '-', "
            "1-63 chars, not starting or ending with '-'"
        )
    if name in ("ca", TOKEN_SIGNER) and kind == "service":
        raise PkiError(f"service name {name!r} is reserved")
    return name


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
    return Step(f"leaf {name}", tuple(args), (crt, key))


def plan(
    out: Path,
    deployment: str,
    services: Sequence[str],
    force: bool,
    exists: Callable[[Path], bool],
) -> list[Step]:
    """The `step` calls still needed. `force` ⇒ all of them; else skip what exists.

    A missing CA forces every leaf too: a leaf signed by a previous, deleted root
    would no longer verify.
    """
    validate_name("deployment", deployment)
    names = [validate_name("service", s) for s in services]
    if len(set(names)) != len(names):
        raise PkiError("duplicate --service")
    ca = ca_step(out, deployment)
    new_ca = force or not all(exists(p) for p in ca.outputs)
    steps = [ca] if new_ca else []
    leaves = [leaf_step(out, deployment, TOKEN_SIGNER, [spiffe_id(deployment, TOKEN_SIGNER)])]
    leaves += [leaf_step(out, deployment, n, leaf_sans(deployment, n)) for n in names]
    for leaf in leaves:
        if new_ca or not all(exists(p) for p in leaf.outputs):
            steps.append(leaf)
    return steps


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
        steps = plan(out, args.deployment, services, args.force, Path.exists)
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
