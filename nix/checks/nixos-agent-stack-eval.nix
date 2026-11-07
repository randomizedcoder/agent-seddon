# nix/checks/nixos-agent-stack-eval.nix
#
# Hermetic EVAL-ONLY gate for the whole-stack NixOS module
# (`nixosModules.agent-stack`). It instantiates two throwaway NixOS configurations
# with the module enabled — loopback (the default) and LAN-published — and asserts
# the units, ClickHouse and Postgres wiring, secret handling and firewall surface it
# promises.
#
# The module closes over the flake's `self` (it reads
# `self.legacyPackages.<system>.agentSeddon`), so this check hands it a STUB `self`
# built from the same per-system deploy attrset the flake exports. Like
# `nixos-agent-postgres-eval` it is PURE EVALUATION — it reads resolved option
# values (ExecStart strings, LoadCredential lists, firewall ports) and never
# builds `system.build.toplevel`, so it needs neither KVM nor a VM. Reading an
# ExecStart string does force its store paths' derivations to be INSTANTIATED (not
# built), which is cheap and keeps the gate offline.
{
  pkgs,
  src,
  agentSeddonDeploy,
}:

let
  lib = pkgs.lib;
  system = pkgs.stdenv.hostPlatform.system;

  stubSelf = {
    outPath = src;
    legacyPackages.${system}.agentSeddon = agentSeddonDeploy;
  };

  evalWith =
    extra:
    (import "${pkgs.path}/nixos/lib/eval-config.nix" {
      inherit system;
      modules = [
        (import ../nixos/agent-stack.nix { self = stubSelf; })
        {
          services.agentSeddon = {
            enable = true;
            credentials."kimi.token" = "/var/lib/secrets/kimi.token";
          };
          system.stateVersion = "24.11";
        }
        extra
      ];
    }).config;

  lo = evalWith { };
  lan = evalWith {
    services.agentSeddon = {
      publicHost = "172.16.50.46";
      openFirewall = true;
    };
  };
  # A host that also imports agent-postgres itself (the module is imported once).
  both = evalWith { imports = [ ../nixos/agent-postgres.nix ]; };
  noPg = evalWith { services.agentSeddon.postgres.enable = false; };
  hdx = evalWith {
    services.agentSeddon = {
      publicHost = "172.16.50.46";
      openFirewall = true;
      hyperdx = {
        enable = true;
        frontendUrl = "http://l2:8080";
      };
    };
  };
  graf = evalWith { services.agentSeddon.grafana.enable = true; };
  grafProm = evalWith {
    services.agentSeddon.grafana = {
      enable = true;
      prometheusUrl = "http://127.0.0.1:9090";
    };
  };
  grafFails =
    extra:
    failedAssertions (evalWith {
      services.agentSeddon = lib.recursiveUpdate { grafana.enable = true; } extra;
    }) != [ ];
  grafDs =
    c:
    let
      st = c.services.grafana.provision.datasources.settings;
    in
    if st == null then [ ] else st.datasources;
  grafCh = lib.findFirst (d: d.uid == "clickhouse") null (grafDs graf);
  hdxFails =
    extra:
    failedAssertions (evalWith {
      services.agentSeddon = lib.recursiveUpdate { hyperdx.enable = true; } extra;
    }) != [ ];
  hdxNoAgents = evalWith {
    services.agentSeddon = {
      hyperdx.enable = true;
      gateway.enable = false;
      sessions.enable = false;
      fleet.enable = false;
    };
  };
  hdxNoFirewall = evalWith {
    services.agentSeddon = {
      publicHost = "172.16.50.46";
      hyperdx.enable = true;
    };
    networking.firewall.enable = false;
  };
  # Loopback publicHost: the containers still bind every address.
  hdxLoopbackNoFirewall = evalWith {
    services.agentSeddon.hyperdx.enable = true;
    networking.firewall.enable = false;
  };
  hdxWarns = c: lib.any (lib.hasInfix "services.agentSeddon.hyperdx: the firewall is off") c.warnings;
  # hyperdx.auth = false: HyperDX's local app mode.
  hdxOpen = evalWith {
    services.agentSeddon.hyperdx = {
      enable = true;
      auth = false;
    };
  };
  hdxOpenNoFirewall = evalWith {
    services.agentSeddon = {
      publicHost = "172.16.50.46";
      hyperdx = {
        enable = true;
        auth = false;
      };
    };
    networking.firewall.enable = false;
  };
  # ...firewall off on a loopback publicHost: the containers still bind every address.
  hdxOpenLoopbackNoFirewall = evalWith {
    services.agentSeddon.hyperdx = {
      enable = true;
      auth = false;
    };
    networking.firewall.enable = false;
  };
  # ...and the l2 shape: LAN, firewall on, UI opened.
  hdxOpenLan = evalWith {
    services.agentSeddon = {
      publicHost = "172.16.50.46";
      openFirewall = true;
      hyperdx = {
        enable = true;
        auth = false;
        frontendUrl = "http://l2:8080";
      };
    };
  };
  hdxLanWarns = c: lib.any (lib.hasInfix "no login; anyone who reaches") c.warnings;
  hdxApp = c: c.virtualisation.oci-containers.containers.agent-seddon-hyperdx-app;
  hdxAppEnv = c: (hdxApp c).environment;
  # An agent's merged settings (what its --config TOML is rendered from).
  agentToml = c: name: c.services.agentSeddon.renderedSettings.${name};
  originsOk =
    o:
    failedAssertions (evalWith {
      services.agentSeddon.extraOrigins = [ o ];
    }) == [ ];

  svc = c: name: c.systemd.services.${name};
  execOf = c: name: toString (svc c name).serviceConfig.ExecStart;
  ports = c: c.networking.firewall.allowedTCPPorts or [ ];
  has = c: p: builtins.elem p (ports c);
  # The module's own failed assertions (e.g. a bad credential name) surface here, not
  # as a throw. Filtered by prefix: a bare eval also trips the stock root-fs/bootloader
  # assertions, which say nothing about this module.
  failedAssertions =
    c: lib.filter (a: !a.assertion && lib.hasPrefix "services.agentSeddon" a.message) c.assertions;

  checks = {
    noFailedAssertions = failedAssertions lo == [ ] && failedAssertions lan == [ ];
    dedicatedUser = lo.users.users ? agent-seddon && lo.users.users.agent-seddon.isSystemUser;
    gatewayServeAll = lib.hasInfix "--serve-all" (execOf lo "agent-seddon-gateway");
    sessionsServe = lib.hasInfix "--serve-sessions" (execOf lo "agent-seddon-sessions");
    fleetServe = lib.hasInfix "--serve-fleet" (execOf lo "agent-seddon-fleet");
    agentsRunAsUser = lib.all (n: (svc lo n).serviceConfig.User == "agent-seddon") [
      "agent-seddon-gateway"
      "agent-seddon-sessions"
      "agent-seddon-fleet"
    ];
    # Secrets travel by path via LoadCredential, never inlined.
    credentialWired = builtins.elem "kimi.token:/var/lib/secrets/kimi.token" (
      lib.toList (svc lo "agent-seddon-fleet").serviceConfig.LoadCredential
    );
    clickhouseNative = lo.services.clickhouse.enable;
    clickhouseLoopback = lo.services.clickhouse.serverConfig.listen_host == "127.0.0.1";
    schemaAfterClickhouse = builtins.elem "clickhouse.service" (svc lo "agent-seddon-clickhouse-schema")
    .after;
    fleetNeedsSchema = builtins.elem "agent-seddon-clickhouse-schema.service" (svc lo "agent-seddon-fleet")
    .requires;
    postgresNative = lo.services.postgresql.enable && lo.services.agentPostgres.enable;
    postgresLoopback = lo.services.postgresql.settings.listen_addresses == "127.0.0.1";
    # The generated password reaches postgres-owned postgresql-setup by credential, not by path.
    postgresPasswordByCredential =
      builtins.elem "agent-pg-password:/var/lib/agent-seddon/postgres/password" (svc lo "postgresql-setup")
      .serviceConfig.LoadCredential
      && lib.hasPrefix "/run/credentials/postgresql-setup.service/" (
        toString lo.services.agentPostgres.passwordFile
      );
    postgresSetupAfterCreds = builtins.elem "agent-seddon-postgres-creds.service" (svc lo "postgresql-setup")
    .requires;
    # The role password is set after ensureUsers creates the role, not in postgresql's postStart.
    postgresPasswordInSetup = lib.hasInfix ''ALTER ROLE "agent"'' (svc lo "postgresql-setup").script;
    fleetNeedsPostgres = builtins.elem "postgresql-setup.service" (svc lo "agent-seddon-fleet")
    .requires;
    dedupesWithAgentPostgres = both.services.postgresql.enable;
    postgresOptional =
      !noPg.services.postgresql.enable
      && !(builtins.elem "postgresql-setup.service" (svc noPg "agent-seddon-fleet").requires);
    # Extra CORS origins are exact: a wildcard, path or junk is refused at eval.
    extraOriginsAccepted = lib.all originsOk [
      "http://l2:8092"
      "https://agent.example.org"
      "http://10.0.0.5:8092"
    ];
    extraOriginsRejected = lib.all (o: !originsOk o) [
      "*"
      "null"
      "http://*.example.org"
      "http://l2:8092/"
      "http://l2:8092/../x"
      "l2:8092"
      "javascript://x"
      "http://l2:8092, http://evil"
      ""
    ];
    grpcWebUnit = lo.systemd.services ? agent-seddon-grpc-web;
    portalUnit = lib.hasInfix "--port 8092" (execOf lo "agent-seddon-portal");
    # Loopback default ⇒ nothing opened.
    firewallClosedByDefault = ports lo == [ ];
    portalLoopbackByDefault = lib.hasInfix "--host 127.0.0.1" (execOf lo "agent-seddon-portal");
    # LAN ⇒ portal + the three grpc-web listeners opened; REST (8094) and ClickHouse stay closed.
    lanOpensPublic = lib.all (has lan) [
      8090
      8091
      8092
      8093
    ];
    lanKeepsInternalClosed = !(has lan 8094) && !(has lan 9000) && !(has lan 8123) && !(has lan 5432);
    portalLanBind = lib.hasInfix "--host 0.0.0.0" (execOf lan "agent-seddon-portal");
    # HyperDX: opt-in; three host-network containers, the env rendered off-store first.
    hyperdxOffByDefault =
      lo.virtualisation.oci-containers.containers == { }
      && !(lo.systemd.services ? agent-seddon-hyperdx-env);
    hyperdxContainers = lib.all (n: hdx.virtualisation.oci-containers.containers ? ${n}) [
      "agent-seddon-mongo"
      "agent-seddon-hyperdx-app"
      "agent-seddon-hyperdx-otel"
    ];
    hyperdxMongoLoopback =
      hdx.virtualisation.oci-containers.containers.agent-seddon-mongo.cmd == [
        "--bind_ip"
        "127.0.0.1"
      ];
    hyperdxFrontendUrl =
      hdx.virtualisation.oci-containers.containers.agent-seddon-hyperdx-app.environment.FRONTEND_URL
      == "http://l2:8080";
    # Passwords reach the containers by env file, never the store-rendered environment.
    hyperdxSecretsByFile =
      let
        cs = hdx.virtualisation.oci-containers.containers;
      in
      !(cs.agent-seddon-hyperdx-app.environment ? DEFAULT_CONNECTIONS)
      && !(cs.agent-seddon-hyperdx-otel.environment ? CLICKHOUSE_PASSWORD)
      && cs.agent-seddon-hyperdx-app.environmentFiles == [ "/var/lib/agent-seddon/hyperdx/app.env" ]
      && cs.agent-seddon-hyperdx-otel.environmentFiles == [ "/var/lib/agent-seddon/hyperdx/otel.env" ];
    hyperdxAppAfterEnv =
      builtins.elem "agent-seddon-hyperdx-env.service" (svc hdx "podman-agent-seddon-hyperdx-app")
      .requires
      && builtins.elem "agent-seddon-clickhouse-schema.service" (svc hdx "podman-agent-seddon-hyperdx-otel")
      .requires;
    hyperdxEnvAsAgentUser = (svc hdx "agent-seddon-hyperdx-env").serviceConfig.User == "agent-seddon";
    hyperdxKeyTimer = hdx.systemd.timers ? agent-seddon-hyperdx-key;
    # The key copier is the agent user in the agents' sandbox; only the restart unit is
    # root, and it runs one systemctl command, triggered by the key file changing.
    hyperdxKeyUnprivileged =
      let
        k = (svc hdx "agent-seddon-hyperdx-key").serviceConfig;
      in
      k.User == "agent-seddon"
      && k.NoNewPrivileges
      && k.CapabilityBoundingSet == ""
      && k.ProtectSystem == "strict"
      && k.ReadWritePaths == [ "/var/lib/agent-seddon/hyperdx" ];
    hyperdxKeyAfterEnv = builtins.elem "agent-seddon-hyperdx-env.service" (svc hdx "agent-seddon-hyperdx-key")
    .wants;
    hyperdxRestartOnKey =
      hdx.systemd.paths.agent-seddon-hyperdx-restart.pathConfig.PathChanged
      == "/var/lib/agent-seddon/hyperdx/otlp.headers"
      && lib.hasInfix "systemctl try-restart agent-seddon-fleet.service" (
        execOf hdx "agent-seddon-hyperdx-restart"
      )
      && (svc hdx "agent-seddon-hyperdx-restart").serviceConfig.CapabilityBoundingSet == "";
    # No agents ⇒ nothing to restart (an empty `try-restart` would fail every time).
    hyperdxNoAgentsNoRestart =
      !(hdxNoAgents.systemd.services ? agent-seddon-hyperdx-restart)
      && !(hdxNoAgents.systemd.paths ? agent-seddon-hyperdx-restart);
    # Host networking leaves OTLP/API/OpAMP on every address: firewall off on a LAN warns.
    hyperdxWarnsWithoutFirewall = hdxWarns hdxNoFirewall && !(hdxWarns hdx);
    hyperdxWarnsWithoutFirewallOnLoopback = hdxWarns hdxLoopbackNoFirewall;
    # Agents export OTLP with the key file; each names itself.
    hyperdxAgentsExport =
      let
        t = (agentToml hdx "fleet").telemetry;
      in
      t.otlp_endpoint == "http://127.0.0.1:4317"
      && t.otlp_headers_file == "/var/lib/agent-seddon/hyperdx/otlp.headers"
      && t.otel_service_name == "agent-seddon-fleet";
    hyperdxNoExportWhenOff = !((agentToml lo "fleet").telemetry ? otlp_headers_file);
    # Login on by default: the app is not in local mode.
    hyperdxAuthByDefault = !(hdxAppEnv hdx ? IS_LOCAL_APP_MODE);
    # auth = false: local app mode, with HyperDX's exact guard string (anything else is
    # ignored and the login stays).
    hyperdxNoAuthLocalMode =
      ((hdxAppEnv hdxOpen).IS_LOCAL_APP_MODE or "") == "DANGEROUSLY_is_local_app_mode💀";
    # ...which the image's entry.sh would overwrite, so the app starts a copy without
    # that line and refuses to start if the line is not there. The default keeps the
    # image's own entrypoint.
    hyperdxNoAuthEntryStripsOverride =
      let
        a = hdxApp hdxOpen;
        script = lib.last a.cmd;
        line = ''export IS_LOCAL_APP_MODE="REQUIRED_AUTH"'';
      in
      a.entrypoint == "/bin/sh"
      && lib.hasInfix "grep -qxF '${line}' /etc/local/entry.sh ||" script
      && lib.hasInfix "exit 1" script
      && lib.hasInfix "grep -vxF '${line}' /etc/local/entry.sh" script
      && lib.hasInfix "exec sh /tmp/entry-local.sh" script;
    hyperdxAuthKeepsImageEntry = (hdxApp hdx).entrypoint == null && (hdxApp hdx).cmd == [ ];
    # ...no key to copy, so no key timer, key unit or restart-on-key.
    hyperdxNoAuthNoKeyUnits =
      !(hdxOpen.systemd.timers ? agent-seddon-hyperdx-key)
      && !(hdxOpen.systemd.services ? agent-seddon-hyperdx-key)
      && !(hdxOpen.systemd.services ? agent-seddon-hyperdx-restart)
      && !(hdxOpen.systemd.paths ? agent-seddon-hyperdx-restart);
    # ...and the agents still export, with no headers file (a missing one turns the
    # exporter off).
    hyperdxNoAuthAgentsExport =
      let
        t = (agentToml hdxOpen "fleet").telemetry;
      in
      t.otlp_endpoint == "http://127.0.0.1:4317" && !(t ? otlp_headers_file);
    # The firewall-off warning says nothing asks for a credential (OpAMP never does).
    hyperdxNoAuthWarnsOpen =
      lib.all
        (
          c:
          lib.any (lib.hasInfix "neither OTLP nor the API asks for one either") c.warnings
          && lib.any (lib.hasInfix "the UI (8080) is open with no login") c.warnings
        )
        [
          hdxOpenNoFirewall
          hdxOpenLoopbackNoFirewall
        ]
      && lib.any (lib.hasInfix "OpAMP never") hdxNoFirewall.warnings
      && !(lib.any (lib.hasInfix "open with no login") hdxNoFirewall.warnings);
    # LAN + firewall on + no login: the opened UI is warned about; login on, or no LAN,
    # is not.
    hyperdxNoAuthLanWarns = hdxLanWarns hdxOpenLan && !(hdxLanWarns hdx) && !(hdxLanWarns hdxOpen);
    # LAN + firewall ⇒ only the UI opens; OTLP, the API, OpAMP and Mongo stay closed.
    hyperdxUiOpened = has hdx 8080 && !(has lan 8080);
    hyperdxInternalClosed = lib.all (p: !(has hdx p)) [
      4317
      4318
      8000
      4320
      27017
    ];
    hyperdxNeedsClickhouse = hdxFails { clickhouse.enable = false; };
    hyperdxFrontendUrlRejected = lib.all (u: hdxFails { hyperdx.frontendUrl = u; }) [
      ""
      "l2:8080"
      "javascript://x"
      "http://l2:8080/path"
      "http://l2:8080, http://evil"
    ];
    hyperdxDefaultOk = !(hdxFails { });
    # Grafana: off by default; on, it provisions the dashboards dir + a ClickHouse source
    # as the viewer, whose password comes from a credential, never the store.
    grafanaOffByDefault = !lo.services.grafana.enable && grafDs lo == [ ];
    grafanaProvisionsDashboards =
      graf.services.grafana.enable
      && lib.any (
        p: p.name == "agent-seddon" && p.options.path == agentSeddonDeploy.grafanaDashboards
      ) graf.services.grafana.provision.dashboards.settings.providers;
    grafanaClickhouseAsViewer =
      grafCh != null
      && grafCh.jsonData.username == "agent_viewer"
      && grafCh.jsonData.port == 9000
      && grafCh.secureJsonData.password == "$__file{/run/credentials/grafana.service/ch-viewer}";
    grafanaViewerCredential =
      graf.systemd.services.grafana.serviceConfig.LoadCredential == [
        "ch-viewer:/var/lib/agent-seddon/clickhouse/viewer.password"
      ]
      && lib.elem "agent-seddon-clickhouse-creds.service" graf.systemd.services.grafana.after;
    # The plugin is linked into the plugins dir; declarativePlugins stays unset so
    # plugin preinstall (Prometheus on Grafana 13.3+) keeps running.
    grafanaClickhousePlugin =
      graf.services.grafana.declarativePlugins == null
      && lib.any (
        r: lib.hasPrefix "L+ /var/lib/grafana/plugins/grafana-clickhouse-datasource " r
      ) graf.systemd.tmpfiles.rules;
    grafanaNoPrometheusByDefault = lib.all (d: d.type != "prometheus") (grafDs graf);
    grafanaPrometheusWhenAsked = lib.any (
      d: d.uid == "prometheus" && d.url == "http://127.0.0.1:9090"
    ) (grafDs grafProm);
    grafanaNeedsClickhouse = grafFails { clickhouse.enable = false; };
    grafanaPrometheusUrlRejected =
      lib.all (u: grafFails { grafana.prometheusUrl = u; }) [
        ""
        "127.0.0.1:9090"
        "file:///etc/passwd"
        "http://x:9090/api, http://evil"
      ]
      && !(grafFails { });
  };

  failed = lib.filterAttrs (_: v: !v) checks;
in
pkgs.runCommand "nixos-agent-stack-eval" { } (
  if failed == { } then
    ''
      echo "nixosModules.agent-stack evaluates: ${toString (lib.length (lib.attrNames checks))} checks (units, user, credentials, clickhouse, postgres, hyperdx, grafana, firewall loopback+LAN)" > "$out"
    ''
  else
    throw "nixos-agent-stack-eval: unmet assertions: ${lib.concatStringsSep ", " (lib.attrNames failed)}"
)
