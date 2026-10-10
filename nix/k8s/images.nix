# nix/k8s/images.nix — container images for the k8s deployment targets (k8s track K2).
#
# Design: docs/design/k8s/03-images-and-registry.md.
#
# One image serves every agent role (gateway, sessions, fleet, sandbox); the role
# is the command and the ConfigMap, not the image. It is built with
# `dockerTools.streamLayeredImage`:
#   - one layer per store path, so a Rust-only change re-ships just the agent layer;
#   - `$out` is a script that STREAMS the docker-archive tar to stdout, so no
#     multi-GB tarball ever lands in the Nix store. `nix run .#k8s-images` pipes that
#     stream straight into `k3s ctr images import -` (k3s) or `skopeo` (k8s).
#
# The tag is the image's content hash (`tag = null`), so identical inputs always
# give an identical tag. The committed tag lives in `image-tags.nix`, written only
# by `nix run .#k8s-images` — never by the renderer — so an unrelated Rust PR does
# not churn `rendered/` and fail its drift check ([04]/[03]).
#
# The portal-web and Envoy edge images are NOT built here: the edge is deployed at
# K6, so its images are built there, next to where they are used. K3 (the three
# agent roles) needs only this agent image.
{
  pkgs,
  lib,
  versions,
  agent,
}:
let
  uid = "10001";

  # A non-root account for the agent, with no shell. dockerTools.fakeNss hardcodes
  # uid 65534 (nobody); we want a named 10001, so build the passwd/group tree here.
  # A writable /tmp and a home the agent owns come from the same tree.
  userTree = pkgs.runCommand "agent-user-tree" { } ''
    mkdir -p $out/etc $out/home/agent $out/tmp
    echo 'agent:x:${uid}:${uid}:agent:/home/agent:/noshell' > $out/etc/passwd
    echo 'agent:x:${uid}:' > $out/etc/group
    # root entry too, so tools that look up uid 0 during a build step don't fail.
    echo 'root:x:0:0:root:/root:/noshell' >> $out/etc/passwd
    echo 'root:x:0:' >> $out/etc/group
    chmod 1777 $out/tmp
  '';

  agentImage = pkgs.dockerTools.streamLayeredImage {
    name = "agent-seddon/agent";
    # Content-addressed: identical inputs ⇒ identical tag. `image-tags.nix` pins it.
    tag = null;
    contents = [
      agent
      userTree
      pkgs.dockerTools.caCertificates # /etc/ssl/certs/ca-bundle.crt + SSL_CERT_FILE
      pkgs.tzdata
      # `/bin/grpc-health-probe`: the role Deployments' `exec` health probe. The
      # listeners are strict mTLS and the kubelet's native `grpc:` prober can't
      # present a client certificate, so the probe runs in-container with the
      # pod's own `tls-<role>` cert instead (nix/k8s/lib.nix `grpcProbe`).
      versions.grpc-health-probe
    ]
    # bwrap for the Tier-1 sandbox backend (the sandbox role runs the same image).
    ++ lib.optional pkgs.stdenv.isLinux versions.bubblewrap;
    config = {
      Entrypoint = [ "/bin/agent" ];
      User = "${uid}:${uid}";
      WorkingDir = "/home/agent";
      Env = [
        "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt"
        "TZDIR=/share/zoneinfo"
        "HOME=/home/agent"
      ];
    };
  };
in
{
  images = {
    agent = agentImage;
  };

  # The committed tags, as an attrset the renderer (K3) reads. Kept next to this
  # file so a `nix run .#k8s-images` release is one diff.
  tags = {
    agent = agentImage.imageTag;
  };
}
