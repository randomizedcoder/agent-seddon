"""Tests for pki_dev.py: four-class tables (positive_/negative_/corner_/boundary_ plus
adversarial_ for the names that become paths and SANs), a fake `step` for the
orchestration, and — when PKI_DEV_STEP points at a real step-cli (the
`pki-dev-tests` nix check sets it) — an offline end-to-end run plus check-the-checks:
`--verify` must genuinely REJECT a leaf from another CA and a corrupt certificate.

Run: python3 -m unittest test_pki_dev -v
"""

from __future__ import annotations

import io
import json
import os
import subprocess
import stat
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

import pki_dev as P


class FakeStep:
    """Records calls; `certificate create` writes its two output files."""

    def __init__(self, rc: int = 0, write: bool = True, verify_rc: int = 0, renew_rc: int = 1):
        self.calls: list[list[str]] = []
        self.rc, self.write, self.verify_rc, self.renew_rc = rc, write, verify_rc, renew_rc

    def __call__(self, argv):
        argv = list(argv)
        self.calls.append(argv)
        if argv[1:3] == ["certificate", "verify"]:
            return self.verify_rc
        if argv[1:3] == ["certificate", "needs-renewal"]:
            return self.renew_rc
        if self.rc == 0 and self.write:
            Path(argv[4]).write_text("-----BEGIN CERTIFICATE-----\n")
            Path(argv[5]).write_text("-----BEGIN EC PRIVATE KEY-----\n")
        return self.rc


def quiet(fn, *a, **kw):
    with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
        return fn(*a, **kw)


class ValidateName(unittest.TestCase):
    CASES = [
        ("positive_simple", "service", "agent", True),
        ("positive_digits_and_dash", "service", "seam-2", True),
        ("boundary_single_char", "service", "a", True),
        ("boundary_63_chars", "service", "a" * 63, True),
        ("boundary_64_chars", "service", "a" * 64, False),
        ("negative_empty", "service", "", False),
        ("negative_uppercase", "service", "Agent", False),
        ("corner_reserved_ca", "service", "ca", False),
        ("corner_reserved_token_signer", "service", "token-signer", False),
        ("corner_ca_ok_as_deployment", "deployment", "ca", True),
        ("adversarial_traversal", "service", "../etc", False),
        ("adversarial_separator", "service", "a/b", False),
        ("adversarial_leading_dash_option", "service", "-rf", False),
        ("adversarial_trailing_dash", "service", "a-", False),
        ("adversarial_dot_widens_san", "deployment", "evil.example", False),
        ("adversarial_newline", "service", "a\nb", False),
        ("adversarial_nul", "service", "a\x00b", False),
    ]

    def test_cases(self):
        for name, kind, value, ok in self.CASES:
            with self.subTest(name):
                if ok:
                    self.assertEqual(P.validate_name(kind, value), value)
                else:
                    with self.assertRaises(P.PkiError):
                        P.validate_name(kind, value)


class DefaultOut(unittest.TestCase):
    def test_cases(self):
        cases = [
            ("positive_runtime_dir", {"XDG_RUNTIME_DIR": "/run/user/1000"},
             Path("/run/user/1000/agent-seddon/pki")),
            ("negative_unset", {}, None),
            ("corner_empty", {"XDG_RUNTIME_DIR": ""}, None),
            ("adversarial_relative", {"XDG_RUNTIME_DIR": "run/user"}, None),
        ]
        for name, env, want in cases:
            with self.subTest(name):
                if want is None:
                    with self.assertRaises(P.PkiError):
                        P.default_out(env)
                else:
                    self.assertEqual(P.default_out(env), want)


class Sans(unittest.TestCase):
    def test_positive_leaf_sans_cover_loopback_name_and_spiffe(self):
        self.assertEqual(
            P.leaf_sans("dev", "agent"),
            ["localhost", "127.0.0.1", "::1", "agent", "spiffe://agent.dev/svc/agent"],
        )

    def test_positive_leaf_step_passes_every_san_and_the_ca(self):
        s = P.leaf_step(Path("/o"), "dev", "agent", P.leaf_sans("dev", "agent"))
        self.assertEqual(s.args.count("--san"), 5)
        self.assertIn("/o/ca/root.key", s.args)
        self.assertIn("--no-password", s.args)
        self.assertEqual(s.outputs, (Path("/o/agent/cert.pem"), Path("/o/agent/key.pem")))


class ValidateSan(unittest.TestCase):
    CASES = [
        ("positive_ipv4", "172.16.50.46", "172.16.50.46"),
        ("positive_ipv6", "fd00::1", "fd00::1"),
        ("positive_host", "l2", "l2"),
        ("positive_fqdn", "front.example.org", "front.example.org"),
        ("corner_uppercase_host_lowered", "L2.Example", "l2.example"),
        ("corner_ipv6_canonicalised", "FD00:0::1", "fd00::1"),
        ("boundary_63_char_label", "a" * 63 + ".test", "a" * 63 + ".test"),
        ("boundary_64_char_label", "a" * 64 + ".test", None),
        ("boundary_253_chars", ".".join(["a" * 63] * 3 + ["b" * 61]), ".".join(["a" * 63] * 3 + ["b" * 61])),
        ("boundary_254_chars", ".".join(["a" * 63] * 3 + ["b" * 62]), None),
        ("negative_empty", "", None),
        ("negative_trailing_dot", "l2.", None),
        ("negative_numeric_tld_not_an_ip", "1.2.3.999", None),
        ("adversarial_wildcard", "*.example.org", None),
        ("adversarial_scheme", "https://l2", None),
        ("adversarial_port", "l2:8443", None),
        ("adversarial_comma_two_sans", "l2,evil.test", None),
        ("adversarial_space", "l2 evil.test", None),
        ("adversarial_leading_dash_option", "-rf", None),
        ("adversarial_traversal", "../x", None),
        ("adversarial_newline", "l2\nevil", None),
        ("adversarial_nul", "l2\x00", None),
        ("adversarial_huge", "a" * 100_000, None),
    ]

    def test_cases(self):
        for name, value, want in self.CASES:
            with self.subTest(name):
                if want is None:
                    with self.assertRaises(P.PkiError):
                        P.validate_san(value)
                else:
                    self.assertEqual(P.validate_san(value), want)


class Renew(unittest.TestCase):
    CASES = [
        ("positive_30_days", "720h", True),
        ("boundary_one_hour", "1h", True),
        ("boundary_just_under_validity", "2159h", True),
        ("boundary_validity_itself", "2160h", False),
        ("negative_zero", "0h", False),
        ("negative_no_unit", "720", False),
        ("negative_days_unit", "30d", False),
        ("corner_leading_zero", "0720h", False),
        ("corner_fraction", "1.5h", False),
        ("adversarial_negative", "-1h", False),
        ("adversarial_second_flag", "720h --force", False),
        ("adversarial_newline", "720h\n", False),
        ("adversarial_huge", "9" * 10_000 + "h", False),
    ]

    def test_cases(self):
        for name, value, ok in self.CASES:
            with self.subTest(name):
                if ok:
                    self.assertEqual(P.validate_renew(value), value)
                else:
                    with self.assertRaises(P.PkiError):
                        P.validate_renew(value)

    def test_exit_code_cases(self):
        """0 renew, 1 fine; a missing (2) or unreadable (255) leaf is re-minted."""
        for name, rc, want in [
            ("positive_needs_renewal", 0, True),
            ("negative_fine", 1, False),
            ("corner_missing_file", 2, True),
            ("adversarial_corrupt_leaf", 255, True),
        ]:
            with self.subTest(name):
                fake = FakeStep(renew_rc=rc)
                self.assertEqual(P.renewal_due("step", fake, "720h")(Path("/o/front/cert.pem")), want)
                self.assertEqual(
                    fake.calls,
                    [["step", "certificate", "needs-renewal", "/o/front/cert.pem", "--expires-in", "720h"]],
                )


class Plan(unittest.TestCase):
    OUT = Path("/o")

    def names(self, steps):
        return [s.what for s in steps]

    def test_cases(self):
        everything = ["root CA", "leaf token-signer", "leaf agent", "leaf cli"]
        all_files = {p for s in P.plan(self.OUT, "dev", ["agent", "cli"], True, lambda _: False)
                     for p in s.outputs}
        cases = [
            ("positive_fresh_dir_mints_all", set(), False, everything),
            ("positive_force_mints_all", all_files, True, everything),
            ("corner_all_present_is_noop", all_files, False, []),
            ("boundary_one_leaf_missing",
             all_files - {Path("/o/cli/key.pem")}, False, ["leaf cli"]),
            ("corner_missing_ca_reissues_every_leaf",
             all_files - {Path("/o/ca/root.key")}, False, everything),
        ]
        for name, present, force, want in cases:
            with self.subTest(name):
                steps = P.plan(self.OUT, "dev", ["agent", "cli"], force, present.__contains__)
                self.assertEqual(self.names(steps), want)

    def test_san_cases(self):
        """A leaf whose SANs changed is re-minted under the same root."""
        files = {p for s in P.plan(self.OUT, "dev", ["front"], True, lambda _: False) for p in s.outputs}
        default = P.sans_text(P.leaf_sans("dev", "front"))
        host = P.sans_text(P.leaf_sans("dev", "front") + ["l2.test"])
        cases = [
            ("positive_new_san_reissues_the_leaf", {}, ["l2.test"], ["leaf front"]),
            ("positive_same_record_is_noop", {"front": host}, ["l2.test"], []),
            ("corner_legacy_leaf_without_record_is_noop", {}, [], []),
            ("corner_san_dropped_reissues", {"front": host}, [], ["leaf front"]),
            ("corner_default_san_repeated_adds_nothing", {"front": default}, ["127.0.0.1"], []),
            ("boundary_changed_host_reissues", {"front": host}, ["l3.test"], ["leaf front"]),
            ("adversarial_tampered_record_reissues", {"front": "evil.test\n"}, [], ["leaf front"]),
        ]
        for name, records, sans, want in cases:
            with self.subTest(name):
                rec = {self.OUT / n / P.SANS_FILE: t for n, t in records.items()}
                steps = P.plan(self.OUT, "dev", ["front"], False, files.__contains__, sans, rec.get)
                self.assertEqual(self.names(steps), want)

    def test_due_cases(self):
        files = {p for s in P.plan(self.OUT, "dev", ["front"], True, lambda _: False) for p in s.outputs}
        front = self.OUT / "front" / "cert.pem"
        cases = [
            ("positive_due_leaf_reissued_root_kept", {front}, ["leaf front"]),
            ("negative_nothing_due", set(), []),
            ("corner_every_leaf_due", {front, self.OUT / "token-signer" / "cert.pem"},
             ["leaf token-signer", "leaf front"]),
            ("corner_root_cert_due_is_ignored", {self.OUT / "ca" / "root.crt"}, []),
        ]
        for name, due, want in cases:
            with self.subTest(name):
                steps = P.plan(self.OUT, "dev", ["front"], False, files.__contains__, (), lambda _: None, due.__contains__)
                self.assertEqual(self.names(steps), want)

    def test_adversarial_bad_san_rejected_before_planning(self):
        with self.assertRaises(P.PkiError):
            P.plan(self.OUT, "dev", ["front"], False, lambda _: False, ["*.evil.test"])

    def test_negative_duplicate_service(self):
        with self.assertRaises(P.PkiError):
            P.plan(self.OUT, "dev", ["agent", "agent"], False, lambda _: False)

    def test_adversarial_bad_names_rejected_before_planning(self):
        for services, deployment in ((["../x"], "dev"), (["agent"], "a/b")):
            with self.subTest(services=services, deployment=deployment):
                with self.assertRaises(P.PkiError):
                    P.plan(self.OUT, deployment, services, False, lambda _: False)


class Generate(unittest.TestCase):
    def test_positive_writes_files_0600_in_0700_dirs(self):
        with tempfile.TemporaryDirectory() as t:
            out = Path(t) / "pki"
            out.mkdir()
            steps = P.plan(out, "dev", ["agent"], False, Path.exists)
            P.generate(out, steps, "step", FakeStep())
            key = out / "agent" / "key.pem"
            self.assertEqual(stat.S_IMODE(key.stat().st_mode), 0o600)
            self.assertEqual(stat.S_IMODE((out / "agent").stat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(out.stat().st_mode), 0o700)

    def test_positive_records_the_leaf_sans_0600(self):
        with tempfile.TemporaryDirectory() as t:
            out = Path(t)
            P.generate(out, P.plan(out, "dev", ["front"], False, Path.exists, ["l2.test"]), "step", FakeStep())
            rec = out / "front" / P.SANS_FILE
            self.assertEqual(rec.read_text(), P.sans_text(P.leaf_sans("dev", "front") + ["l2.test"]))
            self.assertEqual(stat.S_IMODE(rec.stat().st_mode), 0o600)
            # The record is what the next plan compares: nothing left to do.
            self.assertEqual(P.plan(out, "dev", ["front"], False, Path.exists, ["l2.test"], P.read_record), [])

    def test_negative_step_failure_raises(self):
        with tempfile.TemporaryDirectory() as t:
            out = Path(t)
            steps = P.plan(out, "dev", ["agent"], False, Path.exists)
            with self.assertRaisesRegex(P.PkiError, "root CA"):
                P.generate(out, steps, "step", FakeStep(rc=1))

    def test_corner_success_without_output_raises(self):
        with tempfile.TemporaryDirectory() as t:
            out = Path(t)
            steps = P.plan(out, "dev", ["agent"], False, Path.exists)
            with self.assertRaisesRegex(P.PkiError, "missing"):
                P.generate(out, steps, "step", FakeStep(write=False))

    def test_boundary_force_replaces_existing_files(self):
        with tempfile.TemporaryDirectory() as t:
            out = Path(t)
            fake = FakeStep()
            P.generate(out, P.plan(out, "dev", ["agent"], False, Path.exists), "step", fake)
            (out / "agent" / "cert.pem").write_text("stale")
            P.generate(out, P.plan(out, "dev", ["agent"], True, Path.exists), "step", fake)
            self.assertNotEqual((out / "agent" / "cert.pem").read_text(), "stale")


class Verify(unittest.TestCase):
    def minted(self, t):
        out = Path(t)
        P.generate(out, P.plan(out, "dev", ["agent"], False, Path.exists), "step", FakeStep())
        return out

    def test_positive_all_verify(self):
        with tempfile.TemporaryDirectory() as t:
            self.assertEqual(P.verify(self.minted(t), "step", FakeStep()), [])

    def test_negative_failing_leaf_reported(self):
        with tempfile.TemporaryDirectory() as t:
            failures = P.verify(self.minted(t), "step", FakeStep(verify_rc=1))
            self.assertEqual(len(failures), 2)  # token-signer + agent

    def test_corner_no_root(self):
        with tempfile.TemporaryDirectory() as t:
            self.assertIn("no root CA", P.verify(Path(t), "step", FakeStep())[0])

    def test_corner_no_leaves(self):
        with tempfile.TemporaryDirectory() as t:
            out = Path(t)
            (out / "ca").mkdir()
            (out / "ca" / "root.crt").write_text("x")
            self.assertIn("no leaf", P.verify(out, "step", FakeStep())[0])


class Main(unittest.TestCase):
    def test_positive_mints_and_prints_config(self):
        with tempfile.TemporaryDirectory() as t:
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = P.main(["--out", t], env={}, run=FakeStep())
            self.assertEqual(rc, 0)
            self.assertIn("[grpc.tls]", buf.getvalue())
            self.assertIn(f"{t}/cli/cert.pem", buf.getvalue())

    def test_negative_no_out_and_no_runtime_dir(self):
        self.assertEqual(quiet(P.main, [], env={}, run=FakeStep()), 2)

    def test_adversarial_traversal_service_creates_nothing(self):
        with tempfile.TemporaryDirectory() as t:
            out = Path(t) / "pki"
            fake = FakeStep()
            rc = quiet(P.main, ["--out", str(out), "--service", "../../escape"], env={}, run=fake)
            self.assertEqual(rc, 2)
            self.assertEqual(fake.calls, [])
            self.assertFalse(out.exists())

    def test_adversarial_bad_san_creates_nothing(self):
        with tempfile.TemporaryDirectory() as t:
            out = Path(t) / "pki"
            fake = FakeStep()
            rc = quiet(P.main, ["--out", str(out), "--san", "l2,evil.test"], env={}, run=fake)
            self.assertEqual(rc, 2)
            self.assertEqual(fake.calls, [])
            self.assertFalse(out.exists())

    def test_renew_within_cases(self):
        """A due leaf is re-minted under the kept root; one that isn't due is kept."""
        for name, renew_rc, reminted in [("positive_due", 0, True), ("negative_not_due", 1, False)]:
            with self.subTest(name), tempfile.TemporaryDirectory() as t:
                quiet(P.main, ["--out", t, "--service", "front"], env={}, run=FakeStep())
                (Path(t) / "front" / "cert.pem").write_text("old")
                (Path(t) / "ca" / "root.crt").write_text("root")
                rc = quiet(P.main, ["--out", t, "--service", "front", "--renew-within", "720h"],
                           env={}, run=FakeStep(renew_rc=renew_rc))
                self.assertEqual(rc, 0)
                self.assertEqual((Path(t) / "front" / "cert.pem").read_text() != "old", reminted)
                self.assertEqual((Path(t) / "ca" / "root.crt").read_text(), "root")

    def test_adversarial_bad_renew_window_creates_nothing(self):
        with tempfile.TemporaryDirectory() as t:
            out = Path(t) / "pki"
            fake = FakeStep()
            rc = quiet(P.main, ["--out", str(out), "--renew-within", "1h;rm"], env={}, run=fake)
            self.assertEqual(rc, 2)
            self.assertEqual(fake.calls, [])
            self.assertFalse(out.exists())

    def test_corner_force_keeps_unrelated_files(self):
        with tempfile.TemporaryDirectory() as t:
            keep = Path(t) / "operator-notes.txt"
            keep.write_text("mine")
            quiet(P.main, ["--out", t, "--force"], env={}, run=FakeStep())
            self.assertEqual(keep.read_text(), "mine")


@unittest.skipUnless(os.environ.get("PKI_DEV_STEP"), "set PKI_DEV_STEP to a step-cli binary")
class RealStep(unittest.TestCase):
    """Offline end-to-end with the real step-cli, plus check-the-checks."""

    STEP = os.environ.get("PKI_DEV_STEP", "step")

    def mint(self, out: Path, *extra: str) -> None:
        rc = quiet(P.main, ["--out", str(out), "--step", self.STEP, *extra], env={})
        self.assertEqual(rc, 0)

    def verify_rc(self, out: Path) -> int:
        return quiet(P.main, ["--out", str(out), "--step", self.STEP, "--verify"], env={})

    def test_positive_mint_then_verify(self):
        with tempfile.TemporaryDirectory() as t:
            out = Path(t)
            self.mint(out)
            self.assertEqual(self.verify_rc(out), 0)
            self.assertIn("BEGIN CERTIFICATE", (out / "agent" / "cert.pem").read_text())

    def test_corner_rerun_is_idempotent(self):
        with tempfile.TemporaryDirectory() as t:
            out = Path(t)
            self.mint(out)
            before = (out / "agent" / "cert.pem").read_bytes()
            self.mint(out)
            self.assertEqual((out / "agent" / "cert.pem").read_bytes(), before)

    def sans_of(self, cert: Path) -> list[str]:
        out = subprocess.run(
            [self.STEP, "certificate", "inspect", "--format", "json", str(cert)],
            check=True, capture_output=True, text=True,
        ).stdout
        ext = json.loads(out)["extensions"]["subject_alt_name"]
        return ext.get("dns_names", []) + ext.get("ip_addresses", [])

    def test_positive_new_san_reissues_the_leaf_under_the_same_root(self):
        with tempfile.TemporaryDirectory() as t:
            out = Path(t)
            self.mint(out, "--service", "front")
            root = (out / "ca" / "root.crt").read_bytes()
            self.assertNotIn("172.16.50.46", self.sans_of(out / "front" / "cert.pem"))
            self.mint(out, "--service", "front", "--san", "172.16.50.46", "--san", "l2.test")
            sans = self.sans_of(out / "front" / "cert.pem")
            self.assertIn("172.16.50.46", sans)
            self.assertIn("l2.test", sans)
            self.assertEqual((out / "ca" / "root.crt").read_bytes(), root)
            self.assertEqual(self.verify_rc(out), 0)

    def test_corner_fresh_leaf_is_not_renewed(self):
        with tempfile.TemporaryDirectory() as t:
            out = Path(t)
            self.mint(out, "--service", "front")
            before = (out / "front" / "cert.pem").read_bytes()
            self.mint(out, "--service", "front", "--renew-within", "720h")
            self.assertEqual((out / "front" / "cert.pem").read_bytes(), before)

    def test_check_the_checks_corrupt_leaf_is_renewed(self):
        """`needs-renewal` errors on a corrupt leaf; that must re-mint it, not keep it."""
        with tempfile.TemporaryDirectory() as t:
            out = Path(t)
            self.mint(out, "--service", "front")
            root = (out / "ca" / "root.crt").read_bytes()
            (out / "front" / "cert.pem").write_text("-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n")
            self.assertEqual(self.verify_rc(out), 1)
            self.mint(out, "--service", "front", "--renew-within", "720h")
            self.assertEqual(self.verify_rc(out), 0)
            self.assertEqual((out / "ca" / "root.crt").read_bytes(), root)

    def test_check_the_checks_foreign_leaf_rejected(self):
        with tempfile.TemporaryDirectory() as a, tempfile.TemporaryDirectory() as b:
            self.mint(Path(a))
            self.mint(Path(b))
            (Path(a) / "agent" / "cert.pem").write_bytes((Path(b) / "agent" / "cert.pem").read_bytes())
            self.assertEqual(self.verify_rc(Path(a)), 1)

    def test_check_the_checks_corrupt_cert_rejected(self):
        with tempfile.TemporaryDirectory() as t:
            out = Path(t)
            self.mint(out)
            (out / "cli" / "cert.pem").write_text("-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n")
            self.assertEqual(self.verify_rc(out), 1)


if __name__ == "__main__":
    unittest.main()
