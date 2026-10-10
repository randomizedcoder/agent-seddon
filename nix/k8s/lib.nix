# nix/k8s/lib.nix — the `k8sLib`: a structured-attrset → YAML emitter plus the
# manifest helpers that bake in the hardened defaults every workload gets.
#
# Design: docs/design/k8s/04-manifests-and-gitops.md ("Renderer contract").
#
# Every k8s object is a Nix attribute set (never an interpolated YAML string), so
# the render-tests can walk the structure. `toYAML` serialises one object to a YAML
# document deterministically:
#   - attrset keys come out in Nix's canonical (sorted) order, so the committed
#     `rendered/` tree is byte-stable across machines and nixpkgs bumps;
#   - a multi-line string (e.g. an embedded `agent.toml`) becomes a `|` block
#     scalar, so the ConfigMap stays readable and diffable — the whole point of the
#     committed tree;
#   - scalars are quoted only when YAML would otherwise mis-type them (so `"0"`
#     stays a string sync-wave, `image:tag` stays a string), keeping the diff clean.
#
# It is a pure Nix function: no import-from-derivation, no build-time converter, so
# `nix flake check`'s eval never has to realise a derivation to know the bytes.
{ lib }:
let
  inherit (lib)
    concatStrings
    concatMap
    genList
    isAttrs
    isList
    isString
    isBool
    isInt
    isFloat
    attrNames
    stringLength
    hasInfix
    removePrefix
    any
    ;

  mkIndent = n: concatStrings (genList (_: " ") n);

  # A scalar renders on the same line as its `key:` or `- `.
  isScalar = v: isString v || isBool v || isInt v || isFloat v || v == null;

  isMultiline = s: hasInfix "\n" s;

  # YAML would read these bare tokens as a non-string, so a string that looks like
  # one must be quoted to keep its type. Anything containing a YAML indicator char
  # is quoted too — over-eager (e.g. any `:` quotes) but always safe.
  reservedWords = [
    "true"
    "false"
    "null"
    "yes"
    "no"
    "on"
    "off"
    "True"
    "False"
    "Null"
    "YES"
    "NO"
    "~"
  ];
  specialChars = [
    ":"
    "#"
    "{"
    "}"
    "["
    "]"
    "&"
    "*"
    "!"
    "|"
    ">"
    "'"
    "\""
    "%"
    "@"
    "`"
    ","
  ];
  looksNumeric = s: builtins.match "[-+]?[0-9]+(\\.[0-9]+)?" s != null;

  needsQuote =
    s:
    let
      first = if s == "" then "" else builtins.substring 0 1 s;
      last = if s == "" then "" else builtins.substring (stringLength s - 1) 1 s;
    in
    s == ""
    || builtins.elem s reservedWords
    || looksNumeric s
    || any (c: hasInfix c s) specialChars
    || first == " "
    || last == " "
    || first == "-"
    || first == "?";

  # Escape a string for a YAML double-quoted scalar (backslash then quote).
  quoteString =
    s:
    let
      escaped = builtins.replaceStrings [ "\\" "\"" ] [ "\\\\" "\\\"" ] s;
    in
    "\"${escaped}\"";

  # Render a single-line scalar for the RHS of `key:` or after `- `.
  renderScalarInline =
    v:
    if isBool v then
      (if v then "true" else "false")
    else if isInt v then
      toString v
    else if isFloat v then
      toString v
    else if v == null then
      "null"
    else if needsQuote v then
      quoteString v
    else
      v;

  splitLines = s: lib.splitString "\n" s;

  # A multi-line string value as a `|` block scalar, its body indented by `ind`.
  blockScalarLines =
    ind: s:
    let
      body = if lib.hasSuffix "\n" s then lib.removeSuffix "\n" s else s;
    in
    map (l: if l == "" then "" else "${mkIndent ind}${l}") (splitLines body);

  # Emit a mapping's key/values at indentation `ind`, one list entry per line.
  emitMap =
    ind: attrs:
    concatMap (
      k:
      let
        v = attrs.${k};
        p = mkIndent ind;
      in
      if isScalar v then
        if isString v && isMultiline v then
          [ "${p}${k}: |" ] ++ blockScalarLines (ind + 2) v
        else
          [ "${p}${k}: ${renderScalarInline v}" ]
      else if isList v then
        if v == [ ] then [ "${p}${k}: []" ] else [ "${p}${k}:" ] ++ emitSeq (ind + 2) v
      else if v == { } then
        [ "${p}${k}: {}" ]
      else
        [ "${p}${k}:" ] ++ emitMap (ind + 2) v
    ) (attrNames attrs);

  # Emit a sequence's items at indentation `ind` (the `-` sits at `ind`).
  emitSeq =
    ind: items:
    concatMap (
      item:
      let
        p = mkIndent ind;
      in
      if isScalar item then
        if isString item && isMultiline item then
          [ "${p}- |" ] ++ blockScalarLines (ind + 2) item
        else
          [ "${p}- ${renderScalarInline item}" ]
      else if isList item then
        if item == [ ] then [ "${p}- []" ] else [ "${p}-" ] ++ emitSeq (ind + 2) item
      else if item == { } then
        [ "${p}- {}" ]
      else
        # A mapping item: emit its fields at ind+2, then fold the first field up
        # onto the `-` line so it reads `- key: value`.
        let
          mapLines = emitMap (ind + 2) item;
          firstRest = removePrefix (mkIndent (ind + 2)) (builtins.head mapLines);
        in
        [ "${p}- ${firstRest}" ] ++ builtins.tail mapLines
    ) items;

  # One k8s object → a YAML document string (leading `---`, trailing newline).
  toYAML = obj: concatStrings (map (l: "${l}\n") ([ "---" ] ++ emitMap 0 obj));

  # ── Manifest helpers (the hardened defaults live here, once) ────────────────

  partOf = "agent-seddon";

  # The three standard labels every object carries (gate: the `part-of` label).
  labels = component: {
    "app.kubernetes.io/name" = component;
    "app.kubernetes.io/part-of" = partOf;
    "app.kubernetes.io/component" = component;
  };

  # The ArgoCD sync-wave annotation (a STRING, so it survives as `"0"` not int 0).
  syncWave = n: { "argocd.argoproj.io/sync-wave" = toString n; };

  # Where a role's cert-manager Secret is mounted (and the probe reads it from).
  tlsMountPath = "/etc/agent/tls";

  # A readiness/liveness/startup probe that checks the standard
  # `grpc.health.v1.Health` the agent serves on the role's port.
  #
  # It is an `exec` probe, not the kubelet's native `grpc:` probe: every role
  # listener is strict mTLS (`[grpc.tls] client_ca`), and the native prober dials
  # plaintext with no client certificate, so its handshake is refused before the
  # health service is reached and the pod never goes Ready. Instead the probe runs
  # `grpc-health-probe` (shipped in the image, nix/k8s/images.nix) inside the
  # container against loopback, presenting the pod's own `tls-<role>` certificate
  # (issued with `client auth` usage, components/pki.nix) and verifying the server
  # by the role name, which is a DNS SAN on that same certificate. One mTLS port,
  # no plaintext health surface.
  grpcProbe =
    {
      component,
      port,
      periodSeconds ? 10,
      failureThreshold ? 3,
    }:
    {
      exec.command = [
        "/bin/grpc-health-probe"
        "-addr=127.0.0.1:${toString port}"
        "-tls"
        "-tls-ca-cert=${tlsMountPath}/ca.crt"
        "-tls-client-cert=${tlsMountPath}/tls.crt"
        "-tls-client-key=${tlsMountPath}/tls.key"
        "-tls-server-name=${component}"
        "-connect-timeout=2s"
        "-rpc-timeout=2s"
      ];
      # Covers the probe's own 2s connect + 2s RPC budget (the k8s default is 1s).
      timeoutSeconds = 5;
      inherit periodSeconds failureThreshold;
    };

  # The image's WorkingDir and HOME. The rootfs is read-only, and the agent's default
  # state paths (`.agent/…`, the search index, tool caches) are relative to the cwd or
  # under HOME, so the Deployment mounts a writable emptyDir here. That state is
  # per-pod and ephemeral until K8 wires the shared stores.
  homePath = "/home/agent";

  # The head of every role's `agent.toml`. `[agent]` and `[provider]` are required
  # by the config loader; K3 roles serve their seams' control planes and reach no
  # model, so the provider points at a closed loopback port until K8 wires the real
  # providers (and their keys, docs/design/k8s/07) in.
  roleBaseToml = ''
    # Placeholder model wiring until K8: the loader requires [agent] + [provider],
    # but no K3 role calls a model, so the provider is a closed loopback port.
    [agent]
    provider = "openai-compat"

    [provider]
    base_url = "http://127.0.0.1:1/v1"
    model = "unwired-until-k8"
  '';

  # The single hardened securityContext every non-sandbox container gets.
  hardenedSecurityContext = {
    runAsNonRoot = true;
    readOnlyRootFilesystem = true;
    allowPrivilegeEscalation = false;
    capabilities.drop = [ "ALL" ];
    seccompProfile.type = "RuntimeDefault";
  };

  # A hardened Deployment for one agent role.
  #
  # `args` is the `agent` argv after the entrypoint (e.g. `[ "--serve-all" ]`); the
  # role's ConfigMap is mounted read-only at /etc/agent and its TLS Secret at
  # /etc/agent/tls, and writable emptyDirs give the read-only rootfs a scratch
  # /tmp and a state-holding HOME/cwd (`homePath`). Probes check gRPC health on the role port over the pod's own mTLS cert
  # (`grpcProbe`); requests are always set and only memory is limited (CPU is
  # not, to avoid throttling a bursty first token).
  deployment =
    {
      component,
      namespace,
      image,
      args,
      port,
      metricsPort,
      wave,
      configMapName,
      tlsSecretName,
      replicas ? 1,
      memoryRequest ? "256Mi",
      memoryLimit ? "512Mi",
      cpuRequest ? "100m",
    }:
    {
      apiVersion = "apps/v1";
      kind = "Deployment";
      metadata = {
        name = component;
        inherit namespace;
        labels = labels component;
        annotations = syncWave wave;
      };
      spec = {
        inherit replicas;
        selector.matchLabels."app.kubernetes.io/name" = component;
        template = {
          metadata.labels = labels component;
          spec = {
            containers = [
              {
                name = "agent";
                inherit image;
                args = args ++ [
                  "--config"
                  "/etc/agent/agent.toml"
                ];
                ports = [
                  {
                    name = "grpc";
                    containerPort = port;
                  }
                  {
                    name = "metrics";
                    containerPort = metricsPort;
                  }
                ];
                readinessProbe = grpcProbe { inherit component port; };
                livenessProbe = grpcProbe { inherit component port; };
                startupProbe = grpcProbe {
                  inherit component port;
                  # A slow first start (cold caches, cert mount) gets ~60s.
                  periodSeconds = 5;
                  failureThreshold = 12;
                };
                securityContext = hardenedSecurityContext;
                resources = {
                  requests = {
                    cpu = cpuRequest;
                    memory = memoryRequest;
                  };
                  limits.memory = memoryLimit;
                };
                volumeMounts = [
                  {
                    name = "config";
                    mountPath = "/etc/agent";
                    readOnly = true;
                  }
                  {
                    name = "tls";
                    mountPath = tlsMountPath;
                    readOnly = true;
                  }
                  {
                    name = "tmp";
                    mountPath = "/tmp";
                  }
                  {
                    name = "home";
                    mountPath = homePath;
                  }
                ];
              }
            ];
            volumes = [
              {
                name = "config";
                configMap.name = configMapName;
              }
              {
                name = "tls";
                secret.secretName = tlsSecretName;
              }
              {
                name = "tmp";
                emptyDir = { };
              }
              {
                name = "home";
                emptyDir = { };
              }
            ];
          };
        };
      };
    };

  # A ClusterIP Service publishing the role's gRPC and metrics ports.
  service =
    {
      component,
      namespace,
      port,
      metricsPort,
      wave,
    }:
    {
      apiVersion = "v1";
      kind = "Service";
      metadata = {
        name = component;
        inherit namespace;
        labels = labels component;
        annotations = syncWave wave;
      };
      spec = {
        selector."app.kubernetes.io/name" = component;
        ports = [
          {
            name = "grpc";
            inherit port;
            targetPort = "grpc";
          }
          {
            name = "metrics";
            port = metricsPort;
            targetPort = "metrics";
          }
        ];
      };
    };

  # A ConfigMap carrying one role's `agent.toml` (already rendered to a string).
  configMapFromToml =
    {
      component,
      namespace,
      wave,
      toml,
    }:
    {
      apiVersion = "v1";
      kind = "ConfigMap";
      metadata = {
        name = "${component}-config";
        inherit namespace;
        labels = labels component;
        annotations = syncWave wave;
      };
      data."agent.toml" = toml;
    };

  # The bare Namespace object (wave 0, before anything that lands in it).
  namespace =
    { name }:
    {
      apiVersion = "v1";
      kind = "Namespace";
      metadata = {
        inherit name;
        labels = labels "namespace";
        annotations = syncWave 0;
      };
    };

  # An ArgoCD Application for one committed component directory. The component's own
  # `application.yaml` is excluded so the app-of-apps never re-applies it.
  application =
    {
      component,
      namespace,
      target,
      repoURL,
      revision ? "main",
    }:
    {
      apiVersion = "argoproj.io/v1alpha1";
      kind = "Application";
      metadata = {
        name = component;
        namespace = "argocd";
        labels = labels component;
      };
      spec = {
        project = "default";
        source = {
          inherit repoURL;
          targetRevision = revision;
          path = "rendered/${target}/${component}";
          directory.exclude = "application.yaml";
        };
        destination = {
          server = "https://kubernetes.default.svc";
          inherit namespace;
        };
        syncPolicy = {
          automated = {
            prune = true;
            selfHeal = true;
          };
          syncOptions = [
            "ServerSideApply=true"
            "CreateNamespace=true"
          ];
        };
      };
    };
in
{
  inherit
    toYAML
    labels
    syncWave
    grpcProbe
    roleBaseToml
    deployment
    service
    configMapFromToml
    namespace
    application
    partOf
    ;
}
