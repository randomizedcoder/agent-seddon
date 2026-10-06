#!/usr/bin/env python3
"""Tests for k8s-render-tests — invariants over the rendered manifests (k8s track K3).

Four case classes (positive_/negative_/boundary_/corner_) plus adversarial_ — the latter
being the check-the-checks matrix the repo convention demands: each mutates a copy of a
real manifest and asserts the matching check now fires, so an always-green assertion
fails the build. The real tree is read once from $AGENT_RENDERED_K3S (the nix check
points it at the committed rendered/k3s store path).

Pure stdlib + PyYAML (via render_checks); no network, no cluster.
"""

from __future__ import annotations

import copy
import os
import unittest

import render_checks as rc
from render_checks import Manifest

ROOT = os.environ.get("AGENT_RENDERED_K3S")


class RenderInvariants(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        assert ROOT, "set AGENT_RENDERED_K3S to the rendered/k3s directory"
        cls.ms = rc.load_target(ROOT)

    def _mutate(self, pred, mutate) -> list[Manifest]:
        """A deep copy of the whole tree with the first manifest matching `pred` mutated
        in place — so whole-tree cross-reference checks still see the rest intact."""
        ms = [Manifest(m.path, copy.deepcopy(m.doc)) for m in self.ms]
        mutate(next(m for m in ms if pred(m)))
        return ms

    @staticmethod
    def _is_deploy(m: Manifest) -> bool:
        return m.kind == "Deployment"

    @staticmethod
    def _is_gateway_cert(m: Manifest) -> bool:
        return m.kind == "Certificate" and m.name == "gateway"

    # -- positive: the real tree satisfies every invariant -----------------------------

    def positive_real_tree_is_clean(self) -> None:
        for check in rc.ALL_CHECKS:
            with self.subTest(check=check.__name__):
                self.assertEqual(check(self.ms), [], f"{check.__name__} flagged the committed tree")

    def positive_tree_is_non_empty(self) -> None:
        # A load that silently found nothing would make every check vacuously pass.
        # Today's tree is 19 objects (6 pki + 3x3 role + 4 apps); a floor well under
        # that still catches an empty/near-empty load without being brittle as it grows.
        self.assertGreaterEqual(len(self.ms), 15)

    # -- boundary / corner: shape facts the checks lean on ------------------------------

    def corner_exactly_the_three_role_certificates(self) -> None:
        self.assertEqual(sorted(m.name for m in rc.role_certificates(self.ms)), ["fleet", "gateway", "sessions"])

    def corner_root_ca_is_the_only_isCA(self) -> None:
        cas = [m.name for m in self.ms if m.kind == "Certificate" and m.doc.get("spec", {}).get("isCA")]
        self.assertEqual(cas, [rc.CA_NAME])

    def boundary_applications_carry_no_sync_wave(self) -> None:
        for m in self.ms:
            if m.kind == "Application":
                self.assertNotIn(rc.SYNC_WAVE_ANN, m.annotations, f"{m.path} should not carry a sync wave")

    def boundary_image_tag_is_a_content_hash_string(self) -> None:
        for m in rc.deployments(self.ms):
            for c in rc.containers(m):
                self.assertIsInstance(c.get("image"), str)
                self.assertRegex(c["image"], r"^agent-seddon/agent:[a-z0-9]+$")

    # -- adversarial: each mutation must make its check fire ----------------------------

    def adversarial_runAsNonRoot_false_is_flagged(self) -> None:
        def mut(m):
            rc.containers(m)[0]["securityContext"]["runAsNonRoot"] = False

        self.assertTrue(rc.check_security_context(self._mutate(self._is_deploy, mut)))

    def adversarial_readonly_rootfs_false_is_flagged(self) -> None:
        def mut(m):
            rc.containers(m)[0]["securityContext"]["readOnlyRootFilesystem"] = False

        self.assertTrue(rc.check_security_context(self._mutate(self._is_deploy, mut)))

    def adversarial_allow_privilege_escalation_true_is_flagged(self) -> None:
        def mut(m):
            rc.containers(m)[0]["securityContext"]["allowPrivilegeEscalation"] = True

        self.assertTrue(rc.check_security_context(self._mutate(self._is_deploy, mut)))

    def adversarial_dropping_all_capability_is_flagged(self) -> None:
        def mut(m):
            rc.containers(m)[0]["securityContext"]["capabilities"]["drop"] = []

        self.assertTrue(rc.check_security_context(self._mutate(self._is_deploy, mut)))

    def adversarial_privileged_container_is_flagged(self) -> None:
        def mut(m):
            rc.containers(m)[0]["securityContext"]["privileged"] = True

        self.assertTrue(rc.check_no_privilege(self._mutate(self._is_deploy, mut)))

    def adversarial_sys_admin_capability_is_flagged(self) -> None:
        def mut(m):
            rc.containers(m)[0]["securityContext"].setdefault("capabilities", {}).setdefault("add", []).append("SYS_ADMIN")

        self.assertTrue(rc.check_no_privilege(self._mutate(self._is_deploy, mut)))

    def adversarial_unconfined_seccomp_is_flagged(self) -> None:
        def mut(m):
            rc.containers(m)[0]["securityContext"]["seccompProfile"] = {"type": "Unconfined"}

        self.assertTrue(rc.check_no_privilege(self._mutate(self._is_deploy, mut)))

    def adversarial_missing_liveness_probe_is_flagged(self) -> None:
        def mut(m):
            rc.containers(m)[0].pop("livenessProbe", None)

        self.assertTrue(rc.check_probes(self._mutate(self._is_deploy, mut)))

    def adversarial_missing_part_of_label_is_flagged(self) -> None:
        def mut(m):
            m.doc["metadata"]["labels"].pop("app.kubernetes.io/part-of", None)

        self.assertTrue(rc.check_labels(self._mutate(self._is_deploy, mut)))

    def adversarial_integer_sync_wave_is_flagged(self) -> None:
        def mut(m):
            m.doc["metadata"]["annotations"][rc.SYNC_WAVE_ANN] = 0  # an int, not "0"

        self.assertTrue(rc.check_sync_waves(self._mutate(self._is_deploy, mut)))

    def adversarial_removing_forge_from_exclude_is_flagged(self) -> None:
        def mut(m):
            m.doc["data"]["agent.toml"] = m.doc["data"]["agent.toml"].replace('"sandbox", "pty", "forge"', '"sandbox", "pty"')

        pred = lambda m: m.kind == "ConfigMap" and m.name == "gateway-config"  # noqa: E731
        self.assertTrue(rc.check_exec_seam_exclude(self._mutate(pred, mut)))

    def adversarial_exposing_an_exec_seam_port_is_flagged(self) -> None:
        def mut(m):
            m.doc["spec"]["ports"].append({"name": "sandbox", "port": 50066, "targetPort": "sandbox"})

        self.assertTrue(rc.check_no_exec_seam_exposure(self._mutate(lambda m: m.kind == "Service", mut)))

    def adversarial_non_spiffe_san_scheme_is_flagged(self) -> None:
        def mut(m):
            m.doc["spec"]["uris"] = ["https://agent.l2/svc/gateway"]

        self.assertTrue(rc.check_spiffe_sans(self._mutate(self._is_gateway_cert, mut)))

    def adversarial_wrong_trust_domain_san_is_flagged(self) -> None:
        def mut(m):
            m.doc["spec"]["uris"] = ["spiffe://evil.example/svc/gateway"]

        self.assertTrue(rc.check_spiffe_sans(self._mutate(self._is_gateway_cert, mut)))

    def adversarial_certificate_secret_no_deployment_mounts_is_flagged(self) -> None:
        def mut(m):
            m.doc["spec"]["secretName"] = "tls-rogue"

        self.assertTrue(rc.check_tls_bijection(self._mutate(self._is_gateway_cert, mut)))

    def adversarial_root_ca_without_isca_is_flagged(self) -> None:
        def mut(m):
            m.doc["spec"]["isCA"] = False

        pred = lambda m: m.kind == "Certificate" and m.name == rc.CA_NAME  # noqa: E731
        self.assertTrue(rc.check_ca_chain(self._mutate(pred, mut)))

    def adversarial_role_cert_as_ca_is_flagged(self) -> None:
        # cert-manager has no maxPathLen on Certificate; "root → leaf only" is enforced
        # structurally — no role (leaf) cert may be a CA. A sub-CA attempt must be caught.
        def mut(m):
            m.doc["spec"]["isCA"] = True

        self.assertTrue(rc.check_ca_chain(self._mutate(self._is_gateway_cert, mut)))

    def adversarial_role_cert_wrong_issuer_is_flagged(self) -> None:
        def mut(m):
            m.doc["spec"]["issuerRef"]["name"] = "some-other-issuer"

        self.assertTrue(rc.check_ca_chain(self._mutate(self._is_gateway_cert, mut)))

    def adversarial_application_wrong_source_path_is_flagged(self) -> None:
        def mut(m):
            m.doc["spec"]["source"]["path"] = "rendered/k3s/elsewhere"

        self.assertTrue(rc.check_component_application(self._mutate(lambda m: m.kind == "Application", mut)))

    def adversarial_pem_private_key_in_configmap_is_flagged(self) -> None:
        def mut(m):
            m.doc["data"]["agent.toml"] += "\n-----BEGIN EC PRIVATE KEY-----\nAAAA\n-----END EC PRIVATE KEY-----\n"

        self.assertTrue(rc.check_no_secret_material(self._mutate(lambda m: m.kind == "ConfigMap", mut)))

    def adversarial_password_assignment_in_configmap_is_flagged(self) -> None:
        def mut(m):
            m.doc["data"]["agent.toml"] += '\npassword = "hunter2"\n'

        self.assertTrue(rc.check_no_secret_material(self._mutate(lambda m: m.kind == "ConfigMap", mut)))

    def adversarial_jwt_in_configmap_is_flagged(self) -> None:
        def mut(m):
            m.doc["data"]["agent.toml"] += "\ntoken = eyJhbGciOiJub25lIn0.eyJzdWIiOiJ4In0.sig\n"

        self.assertTrue(rc.check_no_secret_material(self._mutate(lambda m: m.kind == "ConfigMap", mut)))


# rstest-style: register the prefixed methods (unittest only auto-runs test*), so the
# four-class names above are collected without renaming them.
def _register_prefixed() -> None:
    prefixes = ("positive_", "negative_", "boundary_", "corner_", "adversarial_")
    for cls in list(globals().values()):
        if isinstance(cls, type) and issubclass(cls, unittest.TestCase):
            for attr in list(vars(cls)):
                if attr.startswith(prefixes) and callable(getattr(cls, attr)):
                    setattr(cls, "test_" + attr, getattr(cls, attr))


_register_prefixed()


if __name__ == "__main__":
    unittest.main()
