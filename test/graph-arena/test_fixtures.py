"""Check-the-checks: every objective requirement is proven against BOTH its
pass and fail fixtures (R13). A check that accepts its fail fixture is the
classic always-green test bug — it fails this suite, and this suite fails the
build (`graph-arena-tests` in `nix flake check`).

Fixtures are OVERLAYS: a fixture workdir = objective seed/ + fixtures/<name>/.
Needs the objective's toolchains on PATH (the flake check provides them);
hermetic otherwise (GOPROXY=off, shared GOCACHE, no network).
"""

from __future__ import annotations

import shutil
import tempfile
import unittest
from pathlib import Path

import arena_core as core
import driver

OBJECTIVES = sorted(
    p.parent for p in (Path(__file__).parent / "objectives").glob("*/manifest.toml")
)


def compose_workdir(objective_dir: Path, manifest: core.Manifest, overlay: str, dest: Path) -> None:
    dest.mkdir(parents=True)
    if manifest.seed:
        shutil.copytree(objective_dir / manifest.seed, dest, dirs_exist_ok=True)
    shutil.copytree(objective_dir / "fixtures" / overlay, dest, dirs_exist_ok=True)


class FixtureMatrix(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = Path(tempfile.mkdtemp(prefix="arena-fixtures-"))
        cls.gocache = cls.tmp / "gocache"  # shared: no per-case cold stdlib builds

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp, ignore_errors=True)

    def run_requirement_in(self, objective_dir, manifest, overlay, rid) -> core.StepOutcome:
        dest = self.tmp / objective_dir.name / overlay / rid
        compose_workdir(objective_dir, manifest, overlay, dest)
        execute = driver.make_executor(dest, gocache=self.gocache)
        return core.run_requirement(manifest.requirements[rid].steps, execute)

    def test_objectives_exist(self):
        self.assertTrue(OBJECTIVES, "no objectives found")

    def test_every_manifest_loads(self):
        for objective_dir in OBJECTIVES:
            with self.subTest(objective_dir.name):
                m = core.load_manifest((objective_dir / "manifest.toml").read_text())
                self.assertEqual(m.id, objective_dir.name)

    def test_positive_every_check_accepts_its_pass_fixture(self):
        for objective_dir in OBJECTIVES:
            m = core.load_manifest((objective_dir / "manifest.toml").read_text())
            for rid, req in m.requirements.items():
                if not req.steps:
                    continue
                with self.subTest(f"{m.id}/{rid}"):
                    out = self.run_requirement_in(objective_dir, m, "pass", rid)
                    self.assertTrue(out.ok, f"{m.id}/{rid} vs pass fixture: {out.detail}")

    def test_negative_every_check_rejects_its_fail_fixture(self):
        for objective_dir in OBJECTIVES:
            m = core.load_manifest((objective_dir / "manifest.toml").read_text())
            for rid, req in m.requirements.items():
                if not req.steps:
                    continue
                with self.subTest(f"{m.id}/{rid}"):
                    fail_dir = objective_dir / "fixtures" / f"fail-{rid}"
                    # The obligation itself: a steps-carrying requirement WITHOUT
                    # a fail fixture is an untestable check — build failure.
                    self.assertTrue(
                        fail_dir.is_dir(),
                        f"{m.id}/{rid}: missing fixtures/fail-{rid}/ (R13 obligation)",
                    )
                    out = self.run_requirement_in(objective_dir, m, f"fail-{rid}", rid)
                    self.assertFalse(
                        out.ok,
                        f"{m.id}/{rid} ACCEPTED its fail fixture — the check cannot fail",
                    )

    def test_corner_seed_alone_fails_every_check(self):
        # The un-worked seed must never score: if a requirement passes on the
        # bare seed, the agent gets it for free and the check measures nothing.
        for objective_dir in OBJECTIVES:
            m = core.load_manifest((objective_dir / "manifest.toml").read_text())
            dest = self.tmp / objective_dir.name / "seed-only"
            dest.mkdir(parents=True, exist_ok=True)
            if m.seed:
                shutil.copytree(objective_dir / m.seed, dest, dirs_exist_ok=True)
            execute = driver.make_executor(dest, gocache=self.gocache)
            for rid, req in m.requirements.items():
                if not req.steps:
                    continue
                with self.subTest(f"{m.id}/{rid}"):
                    out = core.run_requirement(req.steps, execute)
                    self.assertFalse(out.ok, f"{m.id}/{rid} passes on the bare seed")


if __name__ == "__main__":
    unittest.main()
