# 03 — Images and registry

## Images

| Image | Built from | Contents |
|---|---|---|
| `agent-seddon/agent` | `.#agent` | the `agent` binary, `cacert`, `tzdata`, `bubblewrap`, a passwd entry for uid 10001 |
| `agent-seddon/portal-web` | the portal web build in [`nix/portal/default.nix`](../../../nix/portal/default.nix) | `static-web-server` + the bundle, run with `--cache-control-headers=false` as today |
| Envoy | upstream image, pinned in [`nix/versions.nix`](../../../nix/versions.nix) | unchanged |

- Built with `dockerTools.streamLayeredImage`:
  - one layer per store path, so a Rust change re-ships only the `agent` layer;
  - the output is a script that streams the tarball, so no multi-GB file lands in the store.
- One image serves every agent role (gateway, sessions, fleet, sandbox). The role is the command and
  the ConfigMap.
- `config.User = "10001:10001"`, no shell. The sandbox sidecar still runs the same binary; it gets
  its privileges from the pod spec, not from the image ([08](08-sandbox.md)).
- Tags are the Nix output hash, so the same source always gives the same tag.

### Why tags are committed separately

The manifests name images by tag, and `rendered/` is drift-checked by the gate
([04](04-manifests-and-gitops.md)). If the renderer read the tag straight from the image derivation,
every Rust change would change `rendered/`, and every unrelated PR would fail the drift check.

So the tags live in **`nix/k8s/image-tags.nix`**, and only the release step writes that file
(`nix run .#k8s-images`). The drift check compares `rendered/` against the renderer given the
committed tags. A PR that changes code but does not release leaves `rendered/` alone.

A second check, `k8s-image-tags-fresh`, reports (it does not fail) when the committed tag differs
from the current build. It is a reminder, not a gate: releasing is a decision.

## Getting images onto nodes

| Target | Mechanism |
|---|---|
| k3s | `nix run .#k8s-images -- --import`: `<stream-script> \| sudo k3s ctr -n k8s.io images import -`, then rewrite `image-tags.nix`. Manifests use `imagePullPolicy: IfNotPresent`. No registry. |
| k8s | `nix run .#k8s-images -- --push <registry>`: `skopeo copy docker-archive:/dev/stdin docker://<registry>/agent-seddon/agent:<tag>`. The target overlay sets the registry prefix. |

**The registry for k8s** is Zot, as in nix-k8s-examples:
- in-cluster, with an LB address;
- a TLS certificate from the cluster CA;
- htpasswd on push;
- a pull-through cache for docker.io and registry.k8s.io.

Nodes trust it through containerd `certs.d/<host>/hosts.toml`. The push credential is a local file
and never enters git ([07](07-secrets.md)).

**Ordering hazard.** ArgoCD may sync a merged `rendered/` before the image reaches the node, and the
pod then sits in `ErrImageNeverPull` or `ImagePullBackOff` until it does. The release flow imports or
pushes **before** it opens the PR ([04](04-manifests-and-gitops.md#release-flow)), so the image is
already there when the merge lands.

## Gate

- The images build as part of `nix flake check`.
- The `k8s-image-smoke` check loads the agent image's config JSON and asserts:
  - the user is non-root;
  - the entrypoint is the agent binary;
  - no shell is present.

  It also runs the binary out of the image root with `--version`.
