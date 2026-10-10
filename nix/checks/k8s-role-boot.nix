# nix/checks/k8s-role-boot.nix
#
# Gate: every committed k8s role actually boots and passes its own health probe
# (k8s track K3, docs/design/k8s/04-manifests-and-gitops.md).
#
# `k8s-rendered` proves rendered/ is fresh, `k8s-render-tests` proves its invariants,
# `k8s-kubeconform` proves its schema — none of them run the agent. This one does,
# per role, straight from the COMMITTED manifests:
#   - the ConfigMap's `agent.toml` is the config (only the TLS mount path and the
#     0.0.0.0 bind are rewritten, to a minted test PKI and loopback);
#   - the Deployment's container `args` are the argv;
#   - the cwd/HOME is a fresh writable dir (the pod's `home` emptyDir);
#   - the Deployment's own `readinessProbe.exec.command` (with the probe's image path
#     mapped to the store path) must report SERVING over mTLS;
#   - a plaintext probe — what the kubelet's native `grpc:` prober sends — must be
#     refused, so the listener really is the strict-mTLS one the probe was built for.
# A ConfigMap the config loader rejects, a probe whose flags/SAN don't match the role
# cert, or a role that can't start on a writable HOME all fail here, not on the cluster.
#
# The PKI is minted per build (a throwaway P-256 CA + one leaf per role carrying the
# same SPIFFE URI + DNS SANs and serverAuth+clientAuth EKUs as nix/k8s/components/pki.nix).
{
  pkgs,
  lib,
  src,
  versions,
  agent,
  roles,
}:
pkgs.runCommand "k8s-role-boot"
  {
    nativeBuildInputs = [
      pkgs.coreutils
      pkgs.gnused
      pkgs.openssl
      pkgs.yq-go
      versions.grpc-health-probe
    ];
  }
  ''
    set -euo pipefail
    work="$(mktemp -d)"
    probe_bin="${lib.getExe versions.grpc-health-probe}"

    openssl ecparam -name prime256v1 -genkey -noout -out "$work/ca.key"
    openssl req -x509 -new -key "$work/ca.key" -subj "/CN=k8s-role-boot-ca" \
      -days 1 -out "$work/ca.crt"

    for role in ${lib.concatStringsSep " " roles}; do
      dir="${src}/rendered/k3s/$role"
      tls="$work/$role/tls"
      home="$work/$role/home"
      mkdir -p "$tls" "$home"

      # The role's leaf, shaped like its cert-manager Certificate.
      cp "$work/ca.crt" "$tls/ca.crt"
      openssl ecparam -name prime256v1 -genkey -noout -out "$tls/tls.key"
      openssl req -new -key "$tls/tls.key" -subj "/CN=$role" -out "$work/$role/leaf.csr"
      printf '%s\n' \
        "subjectAltName=URI:spiffe://agent.l2/svc/$role,DNS:$role,DNS:$role.agent-seddon,DNS:$role.agent-seddon.svc" \
        "extendedKeyUsage=serverAuth,clientAuth" \
        "keyUsage=critical,digitalSignature" \
        "basicConstraints=CA:FALSE" > "$work/$role/ext.cnf"
      openssl x509 -req -in "$work/$role/leaf.csr" -CA "$work/ca.crt" -CAkey "$work/ca.key" \
        -CAcreateserial -days 1 -extfile "$work/$role/ext.cnf" -out "$tls/tls.crt" 2>/dev/null

      # The committed config, re-pointed at the test PKI and loopback.
      yq -e '.data["agent.toml"]' "$dir/configmap-$role.yaml" \
        | sed -e "s#/etc/agent/tls#$tls#g" -e 's#"0\.0\.0\.0:#"127.0.0.1:#g' \
        > "$work/$role/agent.toml"

      deploy="$dir/deployment-$role.yaml"
      mapfile -t args < <(yq -e '.spec.template.spec.containers[0].args[]' "$deploy" \
        | sed "s#^/etc/agent/agent.toml\$#$work/$role/agent.toml#")
      mapfile -t probe < <(yq -e '.spec.template.spec.containers[0].readinessProbe.exec.command[]' "$deploy" \
        | sed -e "s#^/bin/grpc-health-probe\$#$probe_bin#" -e "s#/etc/agent/tls#$tls#g")
      addr="$(printf '%s\n' "''${probe[@]}" | sed -n 's/^-addr=//p')"
      [ -n "$addr" ] || { echo "FAIL: [$role] the readiness probe names no -addr" >&2; exit 1; }

      echo "k8s-role-boot: [$role] agent ''${args[*]}"
      ( cd "$home" && HOME="$home" exec ${agent}/bin/agent "''${args[@]}" ) \
        > "$work/$role/agent.log" 2>&1 &
      pid=$!

      ok=0
      last=""
      for _ in $(seq 1 90); do
        if ! kill -0 "$pid" 2>/dev/null; then
          echo "FAIL: [$role] the agent exited during startup:" >&2
          tail -20 "$work/$role/agent.log" >&2
          exit 1
        fi
        if last="$("''${probe[@]}" 2>&1)" && [ "$last" = "status: SERVING" ]; then
          ok=1
          break
        fi
        sleep 1
      done
      if [ "$ok" -ne 1 ]; then
        echo "FAIL: [$role] the Deployment's readiness probe never reported SERVING (last: $last)" >&2
        tail -20 "$work/$role/agent.log" >&2
        kill "$pid" || true
        exit 1
      fi
      echo "k8s-role-boot: [$role] readiness probe argv -> SERVING over mTLS"

      if "$probe_bin" -addr="$addr" -connect-timeout=2s -rpc-timeout=2s >/dev/null 2>&1; then
        echo "FAIL: [$role] a plaintext health probe was served; the listener is not strict mTLS" >&2
        kill "$pid" || true
        exit 1
      fi
      echo "k8s-role-boot: [$role] plaintext probe refused"

      kill "$pid" || true
      wait "$pid" 2>/dev/null || true
    done

    echo "OK: ${lib.concatStringsSep ", " roles} boot from their committed manifests and pass their own mTLS probes" > "$out"
  ''
