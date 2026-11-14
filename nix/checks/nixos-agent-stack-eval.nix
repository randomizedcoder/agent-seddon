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
  grafPw = evalWith {
    services.agentSeddon.grafana = {
      enable = true;
      adminPasswordFile = "/run/secrets/grafana-admin";
    };
  };
  grafGen = evalWith {
    services.agentSeddon.grafana = {
      enable = true;
      generateAdminPassword = true;
    };
  };
  grafAdminUnit = c: c.systemd.services.agent-seddon-grafana-admin;
  # Dex (S26): a mock-login config and a GitHub one.
  dexBase = {
    enable = true;
    issuer = "https://l2.example:8443/dex";
    redirectUris = [ "https://l2.example:8443/auth/callback" ];
  };
  dexMock = evalWith {
    services.agentSeddon.dex = dexBase // {
      mockLogin = true;
    };
  };
  dexGh = evalWith {
    services.agentSeddon.dex = dexBase // {
      github = {
        clientId = "Iv1.abc";
        clientSecretFile = "/run/secrets/dex-github";
        orgs = [ "acme" ];
      };
    };
  };
  dexFails =
    extra:
    failedAssertions (evalWith {
      services.agentSeddon.dex = lib.recursiveUpdate (dexBase // { mockLogin = true; }) extra;
    }) != [ ];
  # The front door (S28b): LAN host, Dex behind it, the host's `[auth]` on the gateway.
  fdModule =
    extra:
    { config, ... }:
    {
      services.agentSeddon = lib.recursiveUpdate {
        publicHost = "172.16.50.46";
        openFirewall = true;
        frontDoor.enable = true;
        dex = {
          enable = true;
          mockLogin = true;
        };
        # Host-wide: sessions and fleet verify the door's token too.
        settings.auth = {
          mode = "oidc";
          redirect_uris = [ config.services.agentSeddon.frontDoor.callbackUrl ];
          issuers = [ config.services.agentSeddon.dex.issuerSettings ];
        };
      } extra;
    };
  fd = evalWith (fdModule { });
  fdFile = evalWith (fdModule {
    frontDoor = {
      certFile = "/var/lib/acme/l2/fullchain.pem";
      keyFile = "/run/secrets/front-door.key";
    };
  });
  fdFails = extra: failedAssertions (evalWith (fdModule extra)) != [ ];
  # S26b: the door's gate creates the GitHub App, and Dex signs in with its client.
  ghAppOn = {
    dex.mockLogin = false;
    githubApp.fromSetup = true;
  };
  ghApp = evalWith (fdModule ghAppOn);
  ghAppNamed = evalWith (
    fdModule (
      lib.recursiveUpdate ghAppOn {
        githubApp = {
          org = "acme-co";
          appName = "seddon l2";
          webhookUrl = "https://hooks.example/webhooks/github";
        };
      }
    )
  );
  # l2: the fleet keeps its own App beside the setup App (no `app_dir` mixes the two).
  ghAppFleetOwn = evalWith {
    imports = [
      (fdModule ghAppOn)
      {
        services.agentSeddon.fleet.settings.github_app = {
          app_id = "5390283";
          private_key_ref = "file:/run/credentials/@unit@/github-app.pem";
        };
      }
    ];
  };
  # Only the key ref named: still the fleet's own App, so no `app_dir`. With no `app_id`
  # the agent then refuses to start (`app_id is required`): fail closed, never filled
  # from the setup App's files.
  ghAppFleetKeyOnly = evalWith {
    imports = [
      (fdModule ghAppOn)
      { services.agentSeddon.fleet.settings.github_app.private_key_ref = "env:K"; }
    ];
  };
  # An `app_dir` the operator set beside an explicit App stays, so the agent refuses it.
  ghAppFleetOwnDir = evalWith {
    imports = [
      (fdModule ghAppOn)
      {
        services.agentSeddon.fleet.settings.github_app = {
          app_id = "5390283";
          app_dir = "/srv/other-app";
        };
      }
    ];
  };
  # The operator wrote the module's own path beside an explicit App: dropped like the
  # module's, so the explicit App wins.
  ghAppFleetOwnSameDir = evalWith {
    imports = [
      (fdModule ghAppOn)
      {
        services.agentSeddon.fleet.settings.github_app = {
          app_id = "5390283";
          private_key_ref = "env:K";
          app_dir = "/var/lib/agent-seddon/github-app";
        };
      }
    ];
  };
  ghAppFails = extra: fdFails (lib.recursiveUpdate ghAppOn extra);
  # S29: the door with the host's own tools beside it (`host` is a plain NixOS module).
  fdWith =
    extra: host:
    evalWith {
      imports = [
        (fdModule extra)
        host
      ];
    };
  fdHostGrafana = {
    services.grafana = {
      enable = true;
      settings.security.secret_key = "$__file{/run/secrets/grafana-secret-key}";
    };
  };
  fdGraf = fdWith { grafana.enable = true; } fdHostGrafana;
  fdHdx =
    hyperdx:
    fdWith {
      hyperdx = {
        enable = true;
      }
      // hyperdx;
    } { };
  fdProm =
    prom:
    fdWith { } {
      services.prometheus = {
        enable = true;
      }
      // prom;
    };
  fdPromWarnings = c: lib.filter (lib.hasInfix "routes") c.warnings;
  fdCreds = c: (svc c "agent-seddon-grpc-web").serviceConfig.LoadCredential or [ ];
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
    # Dashboards: on by default without a login, off with one; through the app's API on
    # loopback, as the agent user, re-run at each app start and template change.
    hyperdxDashboardsWithoutLogin =
      let
        u = svc hdxOpen "agent-seddon-hyperdx-dashboards";
        exec = toString u.serviceConfig.ExecStart;
      in
      lib.hasInfix "/bin/hyperdx-provision --api http://127.0.0.1:${toString agentSeddonDeploy.hyperdx.ports.api} --template /nix/store/" exec
      && lib.hasSuffix "-agent-seddon-hyperdx-dashboards.json" exec
      && u.serviceConfig.User == "agent-seddon"
      && u.serviceConfig.Type == "oneshot"
      && builtins.elem "podman-agent-seddon-hyperdx-app.service" u.wantedBy
      && builtins.elem "podman-agent-seddon-hyperdx-app.service" u.after
      && builtins.elem "podman-agent-seddon-hyperdx-app.service" u.partOf
      && u.restartTriggers != [ ];
    hyperdxNoDashboardsWithLogin = !(hdx.systemd.services ? agent-seddon-hyperdx-dashboards);
    hyperdxNoDashboardsWhenOff = !(lo.systemd.services ? agent-seddon-hyperdx-dashboards);
    # ...and asking for them with a login is refused (the API would 401).
    hyperdxDashboardsNeedNoLogin = hdxFails { hyperdx.dashboards = true; };
    hyperdxDashboardsOptOut =
      let
        c = evalWith {
          services.agentSeddon.hyperdx = {
            enable = true;
            auth = false;
            dashboards = false;
          };
        };
      in
      !(c.systemd.services ? agent-seddon-hyperdx-dashboards);
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
      "http://l2:65536"
      "http://l2:0"
      "http://l2:00000"
    ];
    # ...but any real port, written with leading zeros or not, is fine.
    hyperdxFrontendUrlPorts = lib.all (u: !(hdxFails { hyperdx.frontendUrl = u; })) [
      "http://l2:65535"
      "http://l2:1"
      "http://l2:08080/"
      "https://l2"
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
      && lib.elem "agent-seddon-clickhouse-creds.service" graf.systemd.services.grafana.after
      && lib.elem "agent-seddon-clickhouse-creds.service" graf.systemd.services.grafana.requires;
    # The plugin is linked into the plugins dir; declarativePlugins stays unset so
    # plugin preinstall keeps running. The dir is grafana's, or tmpfiles refuses the
    # link (an unsafe path transition).
    grafanaClickhousePlugin =
      graf.services.grafana.declarativePlugins == null
      && lib.elem "d /var/lib/grafana/plugins 0750 grafana grafana - -" graf.systemd.tmpfiles.rules
      && lib.any (
        r: lib.hasPrefix "L+ /var/lib/grafana/plugins/grafana-clickhouse-datasource " r
      ) graf.systemd.tmpfiles.rules;
    # Grafana 13.3+ doesn't bundle Prometheus: its plugin is linked (`L`, keeping a
    # preinstalled copy) so an offline host has it; an older Grafana that bundles it
    # gets no link.
    grafanaPrometheusPlugin =
      let
        linked =
          c: lib.any (lib.hasPrefix "L /var/lib/grafana/plugins/prometheus ") c.systemd.tmpfiles.rules;
        withVersion =
          v:
          evalWith {
            services.agentSeddon.grafana.enable = true;
            services.grafana.package = lib.mkForce (
              pkgs.grafana.overrideAttrs {
                version = v;
                # Only `.version` is read; nothing is built.
                __intentionallyOverridingVersion = true;
              }
            );
          };
      in
      linked (withVersion "13.3.0") && linked (withVersion "14.0.1") && !linked (withVersion "13.1.6");
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
        "http://127.0.0.1:99999"
        # A path after the port doesn't hide it.
        "http://127.0.0.1:99999/prometheus"
        "http://127.0.0.1:0/prometheus"
        # A path is allowed (the door's /prometheus); traversal, a query, a fragment,
        # an empty or spaced segment are not.
        "http://127.0.0.1:9090/../etc"
        "http://127.0.0.1:9090/prometheus?x=1"
        "http://127.0.0.1:9090/prometheus#x"
        "http://127.0.0.1:9090//prometheus"
        "http://127.0.0.1:9090/prom etheus"
      ]
      && !(grafFails { })
      && lib.all (u: !(grafFails { grafana.prometheusUrl = u; })) [
        "http://127.0.0.1:9090"
        "http://127.0.0.1:9090/prometheus"
        "http://127.0.0.1:9090/prometheus/"
        "http://127.0.0.1:09090/prometheus"
      ];
    # The panel-query check ships with the dashboards (and only with them).
    grafanaObsCheckInstalled =
      let
        named = c: lib.any (p: (p.name or "") == "agent-seddon-obs-check") c.environment.systemPackages;
      in
      named graf && !(named lo);
    # Without HyperDX (whose collector creates otel_traces) the trace panels are
    # skipped; with it, every panel runs and so does every HyperDX tile, as the viewer.
    # The Prometheus the module provisions for Grafana has its targets checked.
    grafanaObsCheckSections =
      let
        text =
          c:
          (lib.findFirst (p: (p.name or "") == "agent-seddon-obs-check") {
            text = "";
          } c.environment.systemPackages).text;
        has = c: s: lib.hasInfix s (text c);
        withHdx = evalWith {
          services.agentSeddon = {
            grafana.enable = true;
            hyperdx.enable = true;
          };
        };
        withProm = evalWith {
          services.agentSeddon.grafana = {
            enable = true;
            prometheusUrl = "http://127.0.0.1:9090/prometheus";
          };
        };
        ipv6 = evalWith {
          services.agentSeddon.grafana.enable = true;
          services.grafana.settings.server.http_addr = "::1";
        };
        ipv6Bracketed = evalWith {
          services.agentSeddon.grafana.enable = true;
          services.grafana.settings.server.http_addr = "[::1]";
        };
      in
      has graf "--skip-table default.otel_traces "
      && !(has graf "otel_logs")
      && !(has graf "--hyperdx-template")
      && !(has graf "--prometheus")
      && !(has withHdx "--skip-table")
      && has withHdx "--hyperdx-template /nix/store/"
      && has withHdx "--clickhouse-password-file /var/lib/agent-seddon/clickhouse/viewer.password "
      && has withProm "--prometheus http://127.0.0.1:9090/prometheus "
      && has ipv6 "://[::1]:3000"
      && has ipv6Bracketed "://[::1]:3000"
      && !(has ipv6Bracketed "[[")
      && has graf "://127.0.0.1:3000";
    # The dashboards go into the host's Grafana; turning that off while asking for them
    # is refused rather than provisioning into nothing.
    # The admin password: a file, through LoadCredential into both units; the reset
    # reads it from stdin and runs on every Grafana start (partOf).
    grafanaAdminPasswordFromFile =
      failedAssertions grafPw == [ ]
      &&
        grafPw.services.grafana.settings.security.admin_password
        == "$__file{/run/credentials/grafana.service/admin-password}"
      && lib.elem "admin-password:/run/secrets/grafana-admin" grafPw.systemd.services.grafana.serviceConfig.LoadCredential
      &&
        (grafAdminUnit grafPw).serviceConfig.LoadCredential
        == [ "admin-password:/run/secrets/grafana-admin" ]
      && lib.elem "grafana.service" (grafAdminUnit grafPw).partOf
      && lib.elem "grafana.service" (grafAdminUnit grafPw).wantedBy
      && (grafAdminUnit grafPw).serviceConfig.User == "grafana"
      && lib.hasInfix "--password-from-stdin" (grafAdminUnit grafPw).script
      && lib.hasInfix "< \"$CREDENTIALS_DIRECTORY/admin-password\"" (grafAdminUnit grafPw).script
      && !(grafPw.systemd.services ? agent-seddon-grafana-admin-password);
    grafanaAdminPasswordGenerated =
      failedAssertions grafGen == [ ]
      # Restarting Grafana re-runs the generator, so a deleted file is made again.
      && lib.elem "grafana.service" grafGen.systemd.services.agent-seddon-grafana-admin-password.partOf
      && lib.elem "admin-password:/var/lib/agent-seddon-grafana/admin.password" grafGen.systemd.services.grafana.serviceConfig.LoadCredential
      && lib.elem "agent-seddon-grafana-admin-password.service" grafGen.systemd.services.grafana.requires
      &&
        grafGen.systemd.services.agent-seddon-grafana-admin-password.serviceConfig.StateDirectoryMode
        == "0700"
      # Root: the file lands in a root-only directory, out of the agent user's reach.
      && !(grafGen.systemd.services.agent-seddon-grafana-admin-password.serviceConfig ? User);
    # Off by default: no password setting, no units, the viewer credential alone.
    grafanaAdminPasswordOffByDefault =
      !(graf.systemd.services ? agent-seddon-grafana-admin)
      && !(graf.systemd.services ? agent-seddon-grafana-admin-password)
      && graf.services.grafana.settings.security.admin_password == "admin";
    grafanaAdminPasswordRejects =
      lib.all (f: grafFails { grafana.adminPasswordFile = f; }) [
        "relative/pw"
        "${builtins.storeDir}/abc-pw"
        "/run/secrets/a b"
        ""
        "/run/secrets/pw\nExecStart=/bin/sh"
      ]
      && grafFails {
        grafana = {
          adminPasswordFile = "/run/secrets/pw";
          generateAdminPassword = true;
        };
      }
      && !(grafFails { grafana.generateAdminPassword = true; })
      # Each option alone still needs grafana.enable.
      &&
        lib.all
          (
            g:
            failedAssertions (evalWith {
              services.agentSeddon.grafana = g;
            }) != [ ]
          )
          [
            { adminPasswordFile = "/run/secrets/pw"; }
            { generateAdminPassword = true; }
          ]
      # A socket-only Grafana has no HTTP /api/health for the reset to wait on.
      &&
        failedAssertions (evalWith {
          services.agentSeddon.grafana = {
            enable = true;
            generateAdminPassword = true;
          };
          services.grafana.settings.server.protocol = "socket";
        }) != [ ]
      &&
        failedAssertions (evalWith {
          services.agentSeddon.grafana = {
            enable = true;
            generateAdminPassword = true;
          };
          services.grafana.settings.database.type = "postgres";
        }) != [ ]
      # A DSN overrides `type`, so it is refused even with type left at sqlite3.
      &&
        failedAssertions (evalWith {
          services.agentSeddon.grafana = {
            enable = true;
            generateAdminPassword = true;
          };
          services.grafana.settings.database.url = "postgres://grafana@db/grafana";
        }) != [ ];
    dexMockLogin =
      let
        d = dexMock.services.dex;
        st = d.settings;
        client = lib.head st.staticClients;
      in
      d.enable
      && st.issuer == "https://l2.example:8443/dex"
      && st.web.http == "127.0.0.1:5556"
      && st.storage.type == "sqlite3"
      && map (c: c.type) st.connectors == [ "mockCallback" ]
      && client.id == "agent-seddon"
      && client.secretFile == "/var/lib/agent-seddon-dex/agent-client.secret"
      && client.redirectURIs == [ "https://l2.example:8443/auth/callback" ]
      && dexMock.systemd.services.dex.serviceConfig.StateDirectory == "dex"
      && lib.elem "agent-seddon-dex-secret.service" dexMock.systemd.services.dex.requires
      && lib.elem "agent-seddon-dex-secret.service" dexMock.systemd.services.agent-seddon-gateway.requires
      && lib.any (
        e: e == "dex-client:/var/lib/agent-seddon-dex/agent-client.secret"
      ) dexMock.systemd.services.agent-seddon-gateway.serviceConfig.LoadCredential
      # Root writes it, out of the agent user's reach; the agent reads its credential copy.
      && !(dexMock.systemd.services.agent-seddon-dex-secret.serviceConfig ? User)
      && !(lib.any (lib.hasInfix "replace-secret /run/secrets") (
        lib.toList dexMock.systemd.services.dex.serviceConfig.ExecStartPre
      ));
    dexGithub =
      let
        gh = lib.head dexGh.services.dex.settings.connectors;
      in
      gh.type == "github"
      && gh.config.clientID == "Iv1.abc"
      && gh.config.clientSecret == "/run/secrets/dex-github"
      && gh.config.redirectURI == "https://l2.example:8443/dex/callback"
      && gh.config.orgs == [ { name = "acme"; } ]
      && lib.any (lib.hasInfix "replace-secret /run/secrets/dex-github /run/secrets/dex-github /run/dex/config.yaml") (
        lib.toList dexGh.systemd.services.dex.serviceConfig.ExecStartPre
      );
    dexIssuerSettings =
      dexMock.services.agentSeddon.dex.issuerSettings == {
        name = "dex";
        profile = "generic";
        issuer = "https://l2.example:8443/dex";
        audience = "agent-seddon";
        jwks_url = "http://127.0.0.1:5556/dex/keys";
        backchannel_url = "http://127.0.0.1:5556/dex";
        default_tenant = "local";
        require_email_verified = true;
        client_secret = "file:/run/credentials/@unit@/dex-client";
      }
      # An issuer at the host's root: the back-channel has no path either.
      &&
        (evalWith {
          services.agentSeddon.dex = dexBase // {
            mockLogin = true;
            issuer = "https://idp.example";
          };
        }).services.agentSeddon.dex.issuerSettings.backchannel_url == "http://127.0.0.1:5556";
    dexOffByDefault =
      !(lo.services.dex.enable or false) && !(lo.systemd.services ? agent-seddon-dex-secret);
    dexRejects =
      lib.all (i: dexFails { issuer = i; }) [
        ""
        "https://l2.example:8443/dex/"
        "http://l2.example/dex"
        "http://localhost:5556/dex"
        "https://l2.example/dex?x=1"
        "https://l2.example/dex#f"
        "https://u:p@l2.example/dex"
        "javascript:alert(1)"
        # Port out of range (the URL regex admits 1-5 digits).
        "https://l2.example:99999/dex"
        "https://l2.example:0/dex"
        # A loopback issuer on another port than dex.port (5556): sign-in would 404.
        "http://127.0.0.1:9999/dex"
        "http://127.0.0.1/dex"
        # Leading zeros are read as the number: 999 isn't dex.port, 0 is out of range.
        "http://127.0.0.1:00999/dex"
        "https://l2.example:00000/dex"
      ]
      && dexFails { redirectUris = [ ]; }
      && dexFails { mockLogin = false; }
      # mockLogin (from the base) and GitHub together; the pairing rows follow.
      && dexFails {
        github = {
          clientId = "Iv1.abc";
          clientSecretFile = "/run/secrets/x";
        };
      }
      && dexFails {
        mockLogin = false;
        github.clientId = "Iv1.abc";
      }
      &&
        lib.all
          (
            f:
            dexFails {
              mockLogin = false;
              github = {
                clientId = "Iv1.abc";
                clientSecretFile = f;
              };
            }
          )
          [
            "relative"
            "${builtins.storeDir}/abc-secret"
            "/run/secrets/a b"
            "/run/secrets/x\nExecStartPre=/bin/sh"
          ]
      # The client id has `valid_client_id`'s shape (#728 draft).
      &&
        lib.all
          (
            id:
            dexFails {
              mockLogin = false;
              github = {
                clientId = id;
                clientSecretFile = "/run/secrets/x";
              };
            }
          )
          [
            ""
            "."
            "Iv1."
            ".Iv1"
            "Iv1 x"
            "Iv1.x&redirect_uri=https://evil"
            "Iv1\nx"
            (lib.concatStrings (lib.replicate 65 "a"))
          ]
      &&
        lib.all
          (
            id:
            !(dexFails {
              mockLogin = false;
              github = {
                clientId = id;
                clientSecretFile = "/run/secrets/x";
              };
            })
          )
          [
            "Iv1.abc"
            "Iv23liAbC123"
            "a"
            (lib.concatStrings (lib.replicate 64 "a"))
          ]
      && lib.all (o: dexFails { github.orgs = [ o ]; }) [
        "a/b"
        "-x"
        ""
      ]
      # A port too long to be one is refused, not a `lib.toInt` error (#727 draft): the
      # port rule runs even when the shape rule has already failed.
      && dexFails { issuer = "https://l2.example:123456789012345678901/dex"; }
      && dexFails { issuer = "https://l2.example:000000065536/dex"; }
      && !(dexFails { issuer = "https://l2.example:08443/dex"; })
      && !(dexFails { issuer = "http://127.0.0.1:5556/dex"; })
      && !(dexFails { issuer = "http://127.0.0.1:05556/dex"; })
      && !(dexFails { issuer = "https://l2.example:65535/dex"; })
      # The rule follows dex.port, not a constant.
      && !(dexFails {
        issuer = "http://127.0.0.1:9999/dex";
        port = 9999;
      });
    frontDoorOffByDefault =
      !(lo.systemd.services ? agent-seddon-front-door-pki)
      && !(lo.services.agentSeddon.renderedSettings.gateway ? auth)
      && !(has lan 8443);
    frontDoorWiring =
      let
        gw = fd.services.agentSeddon.renderedSettings;
        pki = svc fd "agent-seddon-front-door-pki";
      in
      failedAssertions fd == [ ]
      # The gate on the gateway only, on the port the rendered Envoy dials.
      &&
        gw.gateway.auth.gate == {
          listen = "127.0.0.1:8097";
          issuer = "dex";
          main_origin = "https://172.16.50.46:8443";
        }
      # Sessions and the fleet verify the door's token, but host no gate.
      && gw.sessions.auth.mode == "oidc"
      && gw.fleet.auth.mode == "oidc"
      && !(gw.sessions.auth ? gate)
      && !(gw.fleet.auth ? gate)
      # Dex behind the door, and the gate's callback registered with it.
      && fd.services.agentSeddon.dex.issuer == "https://172.16.50.46:8443/dex"
      && builtins.elem "https://172.16.50.46:8443/auth/callback" fd.services.agentSeddon.dex.redirectUris
      # The dev-PKI leaf for publicHost, renewed daily, handed to Envoy as credentials.
      &&
        fdCreds fd == [
          "front.crt:/var/lib/agent-seddon-front-door/pki/front-door/cert.pem"
          "front.key:/var/lib/agent-seddon-front-door/pki/front-door/key.pem"
        ]
      && builtins.elem "agent-seddon-front-door-pki.service" (svc fd "agent-seddon-grpc-web").requires
      && lib.hasInfix "--san 172.16.50.46 --renew-within 720h" pki.script
      && pki.serviceConfig.StateDirectoryMode == "0700"
      && !(pki.serviceConfig ? User)
      && fd.systemd.timers.agent-seddon-front-door-pki.timerConfig.OnCalendar == "daily"
      && has fd 8443
      && has fd 8444
      # An operator certificate: no dev PKI.
      && failedAssertions fdFile == [ ]
      && !(fdFile.systemd.services ? agent-seddon-front-door-pki)
      &&
        fdCreds fdFile == [
          "front.crt:/var/lib/acme/l2/fullchain.pem"
          "front.key:/run/secrets/front-door.key"
        ];
    frontDoorRejects =
      fdFails { grpcWeb.enable = false; }
      && fdFails { gateway.enable = false; }
      && fdFails { frontDoor.certFile = "/var/lib/acme/l2/fullchain.pem"; }
      && fdFails { frontDoor.keyFile = "/run/secrets/front-door.key"; }
      &&
        lib.all
          (
            k:
            fdFails {
              frontDoor = {
                certFile = "/var/lib/acme/l2/fullchain.pem";
                keyFile = k;
              };
            }
          )
          [
            "relative.key"
            "${builtins.storeDir}/abc-front.key"
            "/run/secrets/a b"
            "/run/secrets/x\nExecStartPre=/bin/sh"
            # LoadCredential's own separator.
            "/run/secrets/a:b"
          ]
      && lib.all (o: fdFails { frontDoor.extraOrigins = [ o ]; }) [
        "http://l2:8443"
        "https://l2:8443/path"
        "https://*.example"
        ""
        # #717: a port past 65535, an empty label, a mixed-case host.
        "https://l2:99999"
        "https://l2:0"
        "https://.l2"
        "https://L2.example"
        "https://l2..example:8443"
      ]
      && lib.all (h: fdFails { publicHost = h; }) [
        "l2 --force"
        "fd00::1"
        "-l2"
        "l2/x"
        # #717: what pki-dev's validate_san refuses must fail here, not at boot.
        "l2..test"
        "999.999.999.999"
        "172.16.050.46"
        "1.2.3"
        "${lib.concatStrings (lib.replicate 64 "a")}.test"
        (lib.concatStringsSep "." (lib.replicate 64 "abc"))
        # 254 chars: one past the cap.
        (lib.concatStringsSep "." (
          lib.replicate 3 (lib.concatStrings (lib.replicate 63 "a"))
          ++ [ (lib.concatStrings (lib.replicate 62 "a")) ]
        ))
        "L2.Example"
        "l2-"
        "l2."
        ""
      ]
      && lib.all (h: !(fdFails { publicHost = h; })) [
        "l2"
        "l2.example"
        "0.0.0.0"
        "255.255.255.255"
        "${lib.concatStrings (lib.replicate 63 "a")}.test"
        # Exactly 253 chars, the cap.
        (lib.concatStringsSep "." (
          lib.replicate 3 (lib.concatStrings (lib.replicate 63 "a"))
          ++ [ (lib.concatStrings (lib.replicate 61 "a")) ]
        ))
        "x-1.y2"
      ]
      # The host's `[auth]` is incomplete.
      && fdFails { gateway.settings.auth.mode = "off"; }
      # Behind the door each agent names the tenant from the token itself.
      && fdFails { sessions.settings.auth.mode = "off"; }
      && fdFails { fleet.settings.auth.mode = "off"; }
      && fdFails { gateway.settings.auth.redirect_uris = [ "https://172.16.50.46:8443/other" ]; }
      && fdFails { frontDoor.issuer = "google"; }
      && !(fdFails { frontDoor.extraOrigins = [ "https://l2.example:8443" ]; });
    # S29 (F6): Grafana signs in from the gate's headers, under /grafana/.
    frontDoorGrafana =
      let
        g = fdGraf.services.grafana.settings;
      in
      failedAssertions fdGraf == [ ]
      && g.server.root_url == "https://172.16.50.46:8443/grafana/"
      && g.server.serve_from_sub_path
      &&
        g."auth.proxy" == {
          enabled = true;
          header_name = "X-Seddon-User";
          header_property = "username";
          headers = "Role:X-Seddon-Role";
          auto_sign_up = true;
          whitelist = "127.0.0.1";
          enable_login_token = false;
        }
      && g.auth.disable_login_form
      && g.security.cookie_secure
      # Break-glass basic auth stays (loopback only: Envoy drops authorization).
      && (g."auth.basic".enabled or true)
      # Off without the door, or without the host's Grafana.
      && !(lo.services.grafana.settings ? "auth.proxy")
      && !((fdWith { } { }).services.grafana.settings ? "auth.proxy")
      # The door's route is to :3000 over plain HTTP.
      &&
        failedAssertions (
          fdWith { } {
            imports = [ fdHostGrafana ];
            services.grafana.settings.server.http_port = 3001;
          }
        ) != [ ]
      &&
        failedAssertions (
          fdWith { } {
            imports = [ fdHostGrafana ];
            services.grafana.settings.server.protocol = "socket";
          }
        ) != [ ]
      # #719: basic auth stays on, so Grafana listens on loopback only.
      &&
        failedAssertions (
          fdWith { } {
            imports = [ fdHostGrafana ];
            services.grafana.settings.server.http_addr = "0.0.0.0";
          }
        ) != [ ];
    # S29 (F6): HyperDX's sign-in is the door's, at the door's HyperDX origin.
    frontDoorHyperdx =
      failedAssertions (fdHdx { }) == [ ]
      && !(fdHdx { }).services.agentSeddon.hyperdx.auth
      && (fdHdx { }).services.agentSeddon.hyperdx.frontendUrl == "https://172.16.50.46:8444"
      &&
        failedAssertions (fdHdx {
          auth = true;
        }) != [ ]
      # #719: the raw UI port has no login behind the door, so only the door opens.
      && !(has (fdHdx { }) 8080)
      && has (fdHdx { }) 8444
      && !(lib.any (lib.hasInfix "open the HyperDX UI") (fdHdx { }).warnings)
      # Without the door HyperDX keeps its own login.
      && (evalWith { services.agentSeddon.hyperdx.enable = true; }).services.agentSeddon.hyperdx.auth
      &&
        failedAssertions (fdHdx {
          auth = false;
          frontendUrl = "http://l2:8080";
        }) != [ ]
      # Without the door the default is unchanged.
      && hdx.services.agentSeddon.hyperdx.frontendUrl == "http://l2:8080"
      &&
        (evalWith { services.agentSeddon.hyperdx.enable = true; }).services.agentSeddon.hyperdx.frontendUrl
        == "http://127.0.0.1:8080";
    # S29: the host's Prometheus is told what the door needs, not changed.
    frontDoorPrometheus =
      lib.length (fdPromWarnings (fdProm { })) == 1
      && lib.hasInfix ''webExternalUrl = "https://172.16.50.46:8443/prometheus/"'' (
        lib.head (fdPromWarnings (fdProm { }))
      )
      &&
        fdPromWarnings (fdProm {
          webExternalUrl = "https://172.16.50.46:8443/prometheus/";
        }) == [ ]
      &&
        lib.length (
          fdPromWarnings (fdProm {
            webExternalUrl = "https://172.16.50.46:8443/prometheus/";
            port = 9091;
          })
        ) == 1
      && fdPromWarnings fd == [ ]
      && (fdProm { }).services.prometheus.webExternalUrl == null;
    # S31: behind the door the bundle keeps no token (the door's cookie signs it in)
    # and links each tool through the door.
    portalCookieMode =
      let
        d = c: c.services.agentSeddon.portal.finalDefines;
        promFd = fdProm {
          listenAddress = "127.0.0.1";
          webExternalUrl = "/prometheus/";
        };
      in
      (d fd).PORTAL_AUTH == "door"
      # A tool the host doesn't run gets no Launch card.
      && (d fd).PORTAL_GRAFANA_URL == ""
      && (d fd).PORTAL_PROMETHEUS_URL == ""
      && (d fdGraf).PORTAL_GRAFANA_URL == "https://172.16.50.46:8443/grafana/"
      && (d promFd).PORTAL_PROMETHEUS_URL == "https://172.16.50.46:8443/prometheus/"
      && (d (fdHdx { })).PORTAL_HYPERDX_URL == "https://172.16.50.46:8444"
      # Without the door nothing changes: config.dart's defaults.
      && !((d lan) ? PORTAL_AUTH)
      && !((d lan) ? PORTAL_GRAFANA_URL)
      # An operator's own link still wins...
      &&
        (d (fdWith { portal.defines.PORTAL_GRAFANA_URL = "https://g.example/"; } { })).PORTAL_GRAFANA_URL
        == "https://g.example/"
      # ...but not a mode that would put a token back in JavaScript.
      && lib.all (m: fdFails { portal.defines.PORTAL_AUTH = m; }) [
        "on"
        "auto"
        "off"
        ""
      ]
      && !(fdFails { portal.defines.PORTAL_AUTH = "door"; })
      # door mode with no door would bounce off a /auth/login nothing serves.
      &&
        failedAssertions (evalWith {
          services.agentSeddon.portal.defines.PORTAL_AUTH = "door";
        }) != [ ]
      &&
        failedAssertions (evalWith {
          services.agentSeddon.portal.defines.PORTAL_AUTH = "on";
        }) == [ ];
    # S30 (F9): behind the door only the door listens beyond loopback, and the host's
    # firewall may open nothing it stands in front of.
    frontDoorLockdown =
      let
        fdHost = host: fdWith { } host;
        refused = host: failedAssertions (fdHost host) != [ ];
        hdxFd = fdHdx { };
      in
      lib.sort (a: b: a < b) (ports fd) == [
        8443
        8444
      ]
      &&
        lib.sort (a: b: a < b) (ports hdxFd) == [
          8443
          8444
        ]
      && lib.hasInfix "--host 127.0.0.1" (execOf fd "agent-seddon-portal")
      # Without the door the LAN binds are unchanged.
      && lib.hasInfix "--host 0.0.0.0" (execOf lan "agent-seddon-portal")
      && has lan 8092
      # The firewall itself, and what bypasses it wholesale.
      && refused { networking.firewall.enable = false; }
      && refused { networking.firewall.trustedInterfaces = [ "eth0" ]; }
      && refused {
        networking.firewall.trustedInterfaces = [
          "lo"
          "tailscale0"
        ];
      }
      # A tool port, globally, in a range, or on one interface.
      && lib.all (p: refused { networking.firewall.allowedTCPPorts = [ p ]; }) [
        3000
        9090
        8080
        8090
        8092
        8094
        4317
        4318
        8000
        4320
        27017
        8123
        9000
        5432
        5556
        8097
        50100
        # The other grpc-web listeners, the agents' gRPC ports, the fleet webhook.
        8091
        8093
        50080
        50086
        50110
      ]
      && refused {
        networking.firewall.allowedTCPPortRanges = [
          {
            from = 8000;
            to = 8100;
          }
        ];
      }
      && refused {
        networking.firewall.allowedTCPPortRanges = [
          {
            from = 3000;
            to = 3000;
          }
        ];
      }
      && refused { networking.firewall.interfaces.eth0.allowedTCPPorts = [ 8080 ]; }
      && refused {
        networking.firewall.interfaces.wg0.allowedTCPPortRanges = [
          {
            from = 9090;
            to = 9091;
          }
        ];
      }
      # The refusal names what it found.
      && lib.any (a: lib.hasInfix "3000 (networking.firewall.interfaces.eth0)" a.message) (
        failedAssertions (fdHost {
          networking.firewall.interfaces.eth0.allowedTCPPorts = [ 3000 ];
        })
      )
      # Prometheus beside the door listens on loopback.
      && refused { services.prometheus.enable = true; }
      && !(refused {
        services.prometheus = {
          enable = true;
          listenAddress = "127.0.0.1";
        };
      })
      # Not tools: ssh, the webhook, the door itself, ranges around them, loopback.
      && !(refused {
        networking.firewall.allowedTCPPorts = [
          22
          8096
          8443
        ];
      })
      && !(refused {
        networking.firewall.allowedTCPPortRanges = [
          {
            from = 8443;
            to = 8444;
          }
          {
            from = 2999;
            to = 2999;
          }
          {
            from = 3001;
            to = 3001;
          }
        ];
      })
      && !(refused { networking.firewall.trustedInterfaces = [ "lo" ]; })
      && !(refused { networking.firewall.interfaces.lo.allowedTCPPorts = [ 3000 ]; })
      # Without the door none of this is the module's business.
      &&
        failedAssertions (evalWith {
          networking.firewall.enable = false;
          networking.firewall.allowedTCPPorts = [ 3000 ];
        }) == [ ];
    # #720 carries: what binds where with the door, the door without Postgres, and tool
    # links kept from before the door.
    frontDoorBindsAndLinks =
      let
        binds = c: c.services.agentSeddon.grpcWeb.finalBinds;
        fdNoPg = fdWith { postgres.enable = false; } { };
        linkWarns = c: lib.filter (lib.hasInfix "is outside the front door") c.warnings;
        withLink = v: fdWith { portal.defines.PORTAL_GRAFANA_URL = v; } { };
        warnsFor = v: linkWarns (withLink v) != [ ];
      in
      # Behind the door only the webhook keeps the LAN bind; without it both follow the host.
      binds fd == {
        host = "127.0.0.1";
        webhookHost = "0.0.0.0";
      }
      &&
        binds lan == {
          host = "0.0.0.0";
          webhookHost = "0.0.0.0";
        }
      &&
        binds lo == {
          host = "127.0.0.1";
          webhookHost = "127.0.0.1";
        }
      # The door needs no Postgres: same two ports, nothing refused.
      && failedAssertions fdNoPg == [ ]
      &&
        lib.sort (a: b: a < b) (ports fdNoPg) == [
          8443
          8444
        ]
      # A link outside the door warns, naming the define; the door's own route doesn't.
      && warnsFor "http://127.0.0.1:3000"
      && lib.any (lib.hasInfix "PORTAL_GRAFANA_URL") (linkWarns (withLink "http://127.0.0.1:3000"))
      && warnsFor "http://172.16.50.46:3000"
      && linkWarns (fdWith { portal.defines.PORTAL_PROMETHEUS_URL = "http://localhost:9090"; } { }) != [ ]
      && !(warnsFor "https://172.16.50.46:8443/grafana/")
      && !(warnsFor "")
      && linkWarns fd == [ ]
      # Adversarial: look-alikes of the door's origin are outside it.
      && warnsFor "https://172.16.50.46:84430/grafana/"
      && warnsFor "https://172.16.50.46:8443.evil.example/grafana/"
      && warnsFor "https://172.16.50.46:8443"
      # Without the door a loopback link is the portal's to rewrite, not a warning.
      &&
        linkWarns (evalWith {
          services.agentSeddon.portal.defines.PORTAL_GRAFANA_URL = "http://127.0.0.1:3000";
        }) == [ ];
    githubAppFromSetup =
      let
        gw = ghApp.services.agentSeddon.renderedSettings;
        dir = "/var/lib/agent-seddon/github-app";
        gh = lib.head ghApp.services.dex.settings.connectors;
        pre = lib.toList ghApp.systemd.services.dex.serviceConfig.ExecStartPre;
        restart = ghApp.systemd.services.agent-seddon-github-app.script;
      in
      failedAssertions ghApp == [ ]
      # The gate serves /auth/setup; empty names stay out of the TOML.
      && gw.gateway.auth.gate.setup == { inherit dir; }
      && !(gw.sessions.auth ? gate)
      # Every agent reads the App from the same files.
      && lib.all (n: gw.${n}.github_app.app_dir == dir) [
        "gateway"
        "sessions"
        "fleet"
      ]
      # Dex's connector names the files; both are replaced with their contents at start.
      && gh.type == "github"
      && gh.config.clientID == "${dir}/client-id"
      && gh.config.clientSecret == "${dir}/client-secret"
      && lib.any (lib.hasInfix "replace-secret ${dir}/client-id ${dir}/client-id /run/dex/config.yaml") pre
      && lib.any (lib.hasInfix "replace-secret ${dir}/client-secret ${dir}/client-secret /run/dex/config.yaml") pre
      # No App yet: Dex waits, and the path unit starts it and restarts the agents.
      && ghApp.systemd.services.dex.unitConfig.ConditionPathExists == "${dir}/app-id"
      # The directory exists before the agents read it, so a missing one is a typo.
      && lib.elem "d ${dir} 0700 agent-seddon agent-seddon - -" ghApp.systemd.tmpfiles.rules
      && ghApp.systemd.paths.agent-seddon-github-app.pathConfig.PathChanged == "${dir}/app-id"
      && lib.elem "multi-user.target" ghApp.systemd.paths.agent-seddon-github-app.wantedBy
      && lib.hasInfix "systemctl restart dex.service" restart
      && lib.all (n: lib.hasInfix "agent-seddon-${n}.service" restart) [
        "gateway"
        "sessions"
        "fleet"
      ]
      # The names reach the gate as given.
      &&
        ghAppNamed.services.agentSeddon.renderedSettings.gateway.auth.gate.setup == {
          inherit dir;
          org = "acme-co";
          app_name = "seddon l2";
          webhook_url = "https://hooks.example/webhooks/github";
        }
      # An agent naming its own App keeps it, with no `app_dir`; the others read the files.
      && (
        let
          own = ghAppFleetOwn.services.agentSeddon.renderedSettings;
          keyOnly = ghAppFleetKeyOnly.services.agentSeddon.renderedSettings;
        in
        lib.all (c: failedAssertions c == [ ]) [
          ghAppFleetOwn
          ghAppFleetKeyOnly
          ghAppFleetOwnDir
          ghAppFleetOwnSameDir
        ]
        &&
          own.fleet.github_app == {
            app_id = "5390283";
            private_key_ref = "file:/run/credentials/@unit@/github-app.pem";
          }
        && own.gateway.github_app.app_dir == dir
        && own.sessions.github_app.app_dir == dir
        && keyOnly.fleet.github_app == { private_key_ref = "env:K"; }
        &&
          ghAppFleetOwnDir.services.agentSeddon.renderedSettings.fleet.github_app.app_dir == "/srv/other-app"
        &&
          ghAppFleetOwnSameDir.services.agentSeddon.renderedSettings.fleet.github_app == {
            app_id = "5390283";
            private_key_ref = "env:K";
          }
      );
    githubAppOffByDefault =
      let
        gw = fd.services.agentSeddon.renderedSettings;
      in
      !(gw.gateway.auth.gate ? setup)
      && !(gw.fleet ? github_app)
      && !(fd.systemd.paths ? agent-seddon-github-app)
      && !(fd.systemd.services.dex.unitConfig ? ConditionPathExists);
    githubAppRefusals =
      # The gate on the door hosts /auth/setup, and Dex signs in with the App.
      (
        failedAssertions (evalWith {
          services.agentSeddon.githubApp.fromSetup = true;
        }) != [ ]
      )
      # One GitHub client for Dex, not two; and not with the mock.
      && ghAppFails {
        dex.github = {
          clientId = "Iv1.abc";
          clientSecretFile = "/run/secrets/dex-github";
        };
      }
      && ghAppFails { dex.mockLogin = true; }
      && ghAppFails { githubApp.org = "a--b"; }
      && ghAppFails { githubApp.org = "../x"; }
      && ghAppFails { githubApp.appName = "a<b>"; }
      && ghAppFails { githubApp.appName = lib.strings.replicate 35 "a"; }
      && ghAppFails { githubApp.webhookUrl = "http://hooks.example/webhooks/github"; }
      && ghAppFails { githubApp.webhookUrl = "https://hooks.example/x?y=1"; }
      && ghAppFails { githubApp.webhookUrl = "https://hooks.example:99999/x"; }
      && ghAppFails { githubApp.webhookUrl = "https://hooks.example:0/x"; }
      && ghAppFails { githubApp.webhookUrl = "https://hooks.example:00000"; }
      && !(ghAppFails { githubApp.webhookUrl = "https://hooks.example:65535/x"; })
      && !(ghAppFails { githubApp.webhookUrl = "https://hooks.example:8443"; })
      && !(ghAppFails { githubApp.appName = lib.strings.replicate 34 "a"; });
    grafanaNeedsHostGrafana =
      failedAssertions (evalWith {
        services.agentSeddon.grafana.enable = true;
        services.grafana.enable = false;
      }) != [ ];
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
