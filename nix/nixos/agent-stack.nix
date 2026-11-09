# nix/nixos/agent-stack.nix
#
# NixOS-native deployment of the whole agent-seddon stack on one host (the l2 box):
#
#   clickhouse                       native `services.clickhouse` + the agent schema,
#                                    per-role passwords, row-level security; also the
#                                    digest ledger
#   postgresql                       `services.agentPostgres` (nixosModules.agent-postgres):
#                                    the transactional stores — roster, post leases,
#                                    scheduler jobs, provider/role/forge/transport cards
#   agent-seddon-gateway             agent --serve-all      (:50100, metrics :9700)
#   agent-seddon-sessions            agent --serve-sessions (:50080, metrics :9630)
#   agent-seddon-fleet               agent --serve-fleet    (:50086, metrics :9636)
#   agent-seddon-grpc-web            Envoy grpc-web bridge  (:8090 gateway, :8091 sessions,
#                                    :8093 fleet, :8094 REST on loopback)
#   agent-seddon-portal              the Flutter web portal (:8092), a store-path bundle
#   podman-agent-seddon-{mongo,      HyperDX (opt-in `hyperdx.enable`): UI :8080, OTLP :4317
#     hyperdx-app,hyperdx-otel}      into the same ClickHouse (otel_* in DB `default`)
#   grafana (the host's)             opt-in `grafana.enable`: the agent-seddon dashboards
#                                    + a ClickHouse data source in folder "agent-seddon"
#
# It replaces the hand-started stack (`portal-redeploy`, `grpc-web-up`, `clickhouse-up`,
# `nohup agent --serve-fleet`): every piece is a unit, built from this flake's pinned
# sources, so a redeploy is `nix flake update agent-seddon && nixos-rebuild switch`.
#
# Exposed as `nixosModules.agent-stack`. In the host's flake (e.g. ~/nixos/desktop/l2):
#
#   inputs.agent-seddon.url = "github:randomizedcoder/agent-seddon";
#   # nixosSystem modules:
#   agent-seddon.nixosModules.agent-stack
#   { services.agentSeddon = {
#       enable = true;
#       publicHost = "172.16.50.46";       # what browsers dial (LAN) — binds 0.0.0.0
#       openFirewall = true;
#       credentials.llm-token = "/root/secrets/llm-token";
#       settings.provider = { base_url = "https://…/v1"; model = "…";
#         api_key_file = "/run/credentials/@unit@/llm-token"; };
#       fleet.roster = [ { id = "xtcp2"; user = "randomizedcoder"; … } ];
#     }; }
#
# SECRETS never enter the Nix store. Each `credentials.<name> = "/host/path"` is handed to
# every agent unit with systemd `LoadCredential` (readable only by that unit's process,
# whatever the host file's owner); a config value of `/run/credentials/@unit@/<name>`
# is rewritten to the unit's own credentials directory. The ClickHouse passwords are
# generated on first start into /var/lib/agent-seddon/clickhouse (0700, the service
# user) by the same `ch_creds.py` the container apps use; the Postgres password and DSN
# likewise into /var/lib/agent-seddon/postgres.
#
# SECURITY: the agents run model-driven tools, and the gateway includes the `bash`/
# `sandbox` seams — reaching :8090 is code execution as the `agent-seddon` user. With
# `publicHost` set to a LAN address the bridge and portal bind 0.0.0.0 and (with
# `openFirewall`) are reachable from the LAN, unauthenticated (`PORTAL_AUTH=off`: the
# config is rendered at build time and cannot fetch a JWKS). Only on a private lab LAN.
{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.agentSeddon;
  system = pkgs.stdenv.hostPlatform.system;
  seddon = self.legacyPackages.${system}.agentSeddon;
  ports = seddon.envoySpec.ports;
  toml = pkgs.formats.toml { };

  stateDir = "/var/lib/agent-seddon";
  chCredsDir = "${stateDir}/clickhouse";
  chEnabled = cfg.clickhouse.enable;
  pgCredsDir = "${stateDir}/postgres";
  pgEnabled = cfg.postgres.enable;
  pg = config.services.agentPostgres;
  hdx = seddon.hyperdx;
  hdxEnabled = cfg.hyperdx.enable;
  # Login on (the default): sign-up mints the ingestion key the collector requires.
  # Off: HyperDX's local app mode, no login and no key.
  hdxAuth = cfg.hyperdx.auth;
  # The app container's start command under auth = false: entry.sh minus its
  # IS_LOCAL_APP_MODE override (busybox sh; the image has no bash).
  hdxEntryOverride = ''export IS_LOCAL_APP_MODE="REQUIRED_AUTH"'';
  hdxLocalEntry = ''
    grep -qxF '${hdxEntryOverride}' /etc/local/entry.sh || {
      echo "agent-seddon: /etc/local/entry.sh no longer sets IS_LOCAL_APP_MODE as expected; refusing to start without a login" >&2
      exit 1
    }
    grep -vxF '${hdxEntryOverride}' /etc/local/entry.sh > /tmp/entry-local.sh
    exec sh /tmp/entry-local.sh
  '';
  hdxDir = "${stateDir}/hyperdx";
  # Grafana's admin password: the operator's file, or one generated here (root, 0600).
  # Not under stateDir: the agent user has no business reading it.
  grafAdminGenDir = "/var/lib/agent-seddon-grafana";
  grafAdminFile =
    if cfg.grafana.adminPasswordFile != null then
      cfg.grafana.adminPasswordFile
    else if cfg.grafana.generateAdminPassword then
      "${grafAdminGenDir}/admin.password"
    else
      null;
  graf = config.services.grafana;
  # Dex (security-hardening S26): on loopback; the agent reaches it there
  # (`backchannel_url`), browsers at the public `dex.issuer`.
  dexCfg = cfg.dex;
  dexLocal = "http://127.0.0.1:${toString dexCfg.port}${dexPath}";
  # The issuer's path ("/dex"): Dex mounts every route under it, on any listener.
  dexPath =
    let
      m = builtins.match "https?://[^/]+(/.*)?" dexCfg.issuer;
    in
    if m == null || builtins.head m == null then "" else builtins.head m;
  # The agent's OAuth client secret at Dex: generated once (root, 0700 dir); Dex gets it
  # through the nixpkgs module's `secretFile`, the agents through `credentials`.
  dexGenDir = "/var/lib/agent-seddon-dex";
  dexClientSecret = "${dexGenDir}/agent-client.secret";
  dexClientId = "agent-seddon";
  # What `grafana cli` needs to find the database: the server's paths and database
  # sections, rendered the way the nixpkgs module renders the server's config.ini.
  grafCliIni =
    (pkgs.formats.ini {
      mkKeyValue = lib.generators.mkKeyValueDefault {
        mkValueString = v: if v == null then "" else lib.generators.mkValueStringDefault { } v;
      } "=";
    }).generate
      "grafana-cli.ini"
      {
        inherit (graf.settings) paths database;
      };
  grafLocalAddr =
    let
      a = graf.settings.server.http_addr;
    in
    if a == "" || a == "0.0.0.0" || a == "::" || a == "[::]" then "127.0.0.1" else a;
  # Written by agent-seddon-hyperdx-key once an account exists; the agents read it at
  # start (a missing file only turns the OTLP exporter off).
  otlpHeadersFile = "${hdxDir}/otlp.headers";

  # The `[config_store]`-backed stores the managed Postgres holds. Each opens its own
  # lazy pool, hence the small `pool_max`: 3 agents × 6 stores × 4 stays under the
  # server's default 100 connections.
  pgSettings = {
    config_store = {
      backend = "postgres";
      dsn_ref = "file:${pgCredsDir}/dsn";
      pool_max = 4;
      migrate_on_start = true;
    };
    registry.store = "postgres";
    scheduler.store = "postgres";
    role.store = "postgres";
    forge_registry.store = "postgres";
    github_install_registry.store = "postgres";
    transport_registry.store = "postgres";
    themes.store = "postgres";
  };

  # A URL's explicit port, if any, is 1-65535 (the URL regexes allow any 1-5 digits).
  portOk =
    u:
    let
      m = builtins.match ".*:0*([0-9]+)/?" u;
    in
    m == null
    || (
      let
        p = lib.toInt (lib.head m);
      in
      p >= 1 && p <= 65535
    );
  isLoopback = h: h == "127.0.0.1" || h == "localhost" || h == "::1";
  lan = !(isLoopback cfg.publicHost);
  bindHost = if lan then "0.0.0.0" else "127.0.0.1";
  urlFor = port: "http://${cfg.publicHost}:${toString port}";

  # The repo's example config is the base: the agent requires most of its sections
  # (`[agent]`, `[provider]`, ...), and it is what a hand-started process runs with. Its
  # relative paths (`.agent/...`) land in each unit's WorkingDirectory. The inline
  # placeholder `provider.api_key` is dropped: inline beats `api_key_file`/`api_key_env`,
  # so keeping it would silently shadow an operator's key (see `withProviderKey`).
  repoConfig =
    let
      t = lib.importTOML "${self}/config/agent.toml";
    in
    t // { provider = removeAttrs t.provider [ "api_key" ]; };

  # No key source configured ⇒ restore the local-Ollama placeholder the agent needs.
  withProviderKey =
    c:
    if
      lib.any (k: c.provider ? ${k}) [
        "api_key"
        "api_key_env"
        "api_key_file"
      ]
    then
      c
    else
      lib.recursiveUpdate c { provider.api_key = "ollama"; };

  # Settings every agent process shares, over the repo config: the prompt library +
  # context files (from this flake's source, read-only: they are versioned with the
  # code), with the managed ClickHouse the telemetry and digest ledger, and with the
  # managed Postgres the transactional stores.
  baseSettings = lib.foldl' lib.recursiveUpdate { } [
    {
      prompts = {
        backend = "file";
        dir = "${self}/prompts";
      };
      context_files.dir = "${self}/context.d";
    }
    (lib.optionalAttrs chEnabled {
      telemetry = {
        enabled = true;
        clickhouse_url = "127.0.0.1:${toString cfg.clickhouse.nativePort}";
        database = "agent";
        user = "agent_writer";
        password_file = "${chCredsDir}/writer.password";
      };
      # Rides the telemetry connection (agent_writer may write agent_turn_digests).
      digest.store = "clickhouse";
    })
    (lib.optionalAttrs pgEnabled pgSettings)
    (lib.optionalAttrs hdxEnabled {
      telemetry = {
        otlp_endpoint = "http://127.0.0.1:${toString hdx.ports.otlpGrpc}";
      }
      # Without a login there is no key, and a missing headers file turns the exporter off.
      // lib.optionalAttrs hdxAuth { otlp_headers_file = otlpHeadersFile; };
    })
  ];

  services = {
    gateway = {
      flag = "--serve-all";
      port = ports.gateway;
      extra = { };
    };
    sessions = {
      flag = "--serve-sessions";
      port = ports.sessions;
      extra = { };
    };
    fleet = {
      flag = "--serve-fleet";
      port = ports.fleet;
      # Each roster row is reviewed in its own checkout under the fleet root, made by
      # the per-row review factory, which needs the local review engine.
      extra.review.backend = "local";
      extra.review_fleet = {
        root = "${stateDir}/fleet/workspaces";
      }
      // (
        if pgEnabled then
          {
            store = "postgres";
          }
          // lib.optionalAttrs (cfg.fleet.roster != null) { seed_file = "${rosterFile}"; }
        else
          {
            store = "file";
            file = "${stateDir}/fleet/roster.json";
          }
      );
    };
  };

  unitName = name: "agent-seddon-${name}";

  # `/run/credentials/@unit@/x` → this unit's credentials dir. Done on the rendered TOML
  # text (not the attrset) so it reaches any nesting depth, including array tables.
  mergedSettings =
    name: svc:
    withProviderKey (
      lib.foldl' lib.recursiveUpdate repoConfig [
        baseSettings
        (lib.optionalAttrs hdxEnabled { telemetry.otel_service_name = unitName name; })
        svc.extra
        cfg.settings
        cfg.${name}.settings
      ]
    );

  configFile =
    name: svc:
    let
      raw = toml.generate "agent-seddon-${name}.toml" (mergedSettings name svc);
    in
    pkgs.runCommand "agent-seddon-${name}.toml" { } ''
      sed 's#/run/credentials/@unit@/#/run/credentials/${unitName name}.service/#g' ${raw} > "$out"
    '';

  rosterFile = pkgs.writeText "agent-seddon-roster.json" (builtins.toJSON cfg.fleet.roster);

  envoyConfig = seddon.mkEnvoyConfig {
    host = bindHost;
    origins = [
      (urlFor ports.portalWeb)
    ]
    ++ lib.optional lan "http://localhost:${toString ports.portalWeb}"
    ++ cfg.extraOrigins;
  };

  webBundle = seddon.mkWebBundle {
    defines = {
      PORTAL_GRPC_WEB_URL = urlFor ports.grpcWeb;
      PORTAL_SESSIONS_GRPC_WEB_URL = urlFor ports.grpcWebSessions;
      PORTAL_FLEET_GRPC_WEB_URL = urlFor ports.grpcWebFleet;
      # Empty hides the Launch card; a loopback URL is rewritten to the page's host.
      PORTAL_HYPERDX_URL = if hdxEnabled then cfg.hyperdx.frontendUrl else "";
    }
    // cfg.portal.defines;
  };

  # Hardening shared by the agent units. Not `ProtectHome`-strict on /nix: the agent
  # shells out to the store's git/rg/go tools and (opt-in) `nix run`. Namespaces stay
  # allowed for the opt-in bubblewrap Sandbox.
  hardening = {
    NoNewPrivileges = true;
    ProtectSystem = "strict";
    ProtectHome = true;
    PrivateTmp = true;
    PrivateDevices = true;
    ProtectKernelTunables = true;
    ProtectKernelModules = true;
    ProtectKernelLogs = true;
    ProtectControlGroups = true;
    ProtectClock = true;
    ProtectHostname = true;
    RestrictRealtime = true;
    RestrictSUIDSGID = true;
    LockPersonality = true;
    SystemCallArchitectures = "native";
    CapabilityBoundingSet = "";
    RestrictAddressFamilies = [
      "AF_INET"
      "AF_INET6"
      "AF_UNIX"
      "AF_NETLINK"
    ];
    UMask = "0077";
  };

  agentUnit =
    name: svc:
    lib.nameValuePair (unitName name) {
      description = "agent-seddon ${name} (agent ${svc.flag})";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [
        "network-online.target"
      ]
      ++ dbUnits;
      requires = dbUnits;
      environment = {
        HOME = "${stateDir}/${name}";
        RUST_LOG = cfg.logLevel;
      };
      # The roster is declarative. With Postgres its rows are upserted at start
      # (`seed_file`), so portal-added rows persist; with the file store the bundle is
      # re-installed, so a runtime Put/Delete lasts until the next restart.
      preStart = lib.optionalString (name == "fleet" && cfg.fleet.roster != null && !pgEnabled) ''
        install -m 0600 ${rosterFile} ${stateDir}/fleet/roster.json
      '';
      serviceConfig = hardening // {
        User = cfg.user;
        Group = cfg.group;
        StateDirectory = [
          "agent-seddon"
          "agent-seddon/${name}"
        ];
        StateDirectoryMode = "0750";
        WorkingDirectory = "${stateDir}/${name}";
        ExecStart = "${cfg.package}/bin/agent ${svc.flag} --config ${configFile name svc}";
        LoadCredential = lib.mapAttrsToList (n: p: "${n}:${p}") cfg.credentials;
        Restart = "on-failure";
        RestartSec = 5;
      };
    };

  enabledAgents = lib.filterAttrs (name: _: cfg.${name}.enable) services;

  # What every agent unit waits for: the ClickHouse schema and the Postgres role.
  dbUnits =
    lib.optional chEnabled "agent-seddon-clickhouse-schema.service"
    ++ lib.optional pgEnabled "postgresql-setup.service";

  agentOpts = name: {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Run `agent ${services.${name}.flag}` as `${unitName name}.service`.";
    };
    settings = lib.mkOption {
      inherit (toml) type;
      default = { };
      description = ''
        Agent TOML settings for this process only, merged over `services.agentSeddon.settings`
        (see config/agent.toml for every key). `/run/credentials/@unit@/<name>` names a
        `credentials` entry.
      '';
    };
  };
in
{
  # A path import, so a host that also imports `nixosModules.agent-postgres` gets one copy.
  imports = [ ./agent-postgres.nix ];

  options.services.agentSeddon = {
    enable = lib.mkEnableOption "the agent-seddon stack (agents, ClickHouse, grpc-web bridge, portal)";

    package = lib.mkOption {
      type = lib.types.package;
      default = seddon.agent-ghapp;
      defaultText = lib.literalExpression "agent-seddon.legacyPackages.\${system}.agentSeddon.agent-ghapp";
      description = ''
        The `agent` build. Defaults to the `forge-ghapp` variant: a plain `agent` keeps every
        `ghapp:` roster row disabled.
      '';
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "agent-seddon";
      description = "System user the agent processes run as (created by this module).";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "agent-seddon";
      description = "Group of `user`.";
    };

    publicHost = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      example = "172.16.50.46";
      description = ''
        The host browsers use to reach the portal and bridge. It is baked into the web
        bundle (endpoints are compile-time) and is the bridge's only CORS origin. A
        loopback value keeps everything on 127.0.0.1 (reach it over an SSH tunnel); any
        other value binds the bridge and portal to 0.0.0.0.
      '';
    };

    extraOrigins = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "http://l2:8092" ];
      description = ''
        More exact origins the bridge allows by CORS, for other names browsers open the
        portal under (a short hostname, a DNS name). Each is `http(s)://host[:port]`, no
        wildcard or path. The bundle still dials `publicHost`, so it must resolve too.
      '';
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Open the portal and enabled grpc-web ports. Only meaningful with a LAN
        `publicHost`; the agents' gRPC ports, ClickHouse and the REST listener stay loopback.
      '';
    };

    logLevel = lib.mkOption {
      type = lib.types.str;
      default = "info";
      description = "`RUST_LOG` for the agent processes.";
    };

    settings = lib.mkOption {
      inherit (toml) type;
      default = { };
      example = lib.literalExpression ''
        {
          agent.provider = "openai-compat";
          provider = { base_url = "https://llm.example/v1"; model = "m";
                       api_key_file = "/run/credentials/@unit@/llm-token"; };
        }
      '';
      description = ''
        Agent TOML settings shared by every agent process (config/agent.toml documents the
        keys). Paths under `/run/credentials/@unit@/` resolve to the unit's `credentials`.
      '';
    };

    credentials = lib.mkOption {
      type = lib.types.attrsOf lib.types.path;
      default = { };
      example = {
        llm-token = "/root/secrets/llm-token";
        github-app-key = "/root/secrets/github-app.pem";
      };
      description = ''
        Secret files handed to every agent unit via systemd `LoadCredential` and read as
        `/run/credentials/@unit@/<name>` in `settings`. Only the host path is in the store.
      '';
    };

    renderedSettings = lib.mkOption {
      type = lib.types.attrsOf lib.types.anything;
      internal = true;
      readOnly = true;
      description = "Each enabled agent's merged settings, before `@unit@` rewriting (for the eval check).";
    };

    gateway = agentOpts "gateway";
    sessions = agentOpts "sessions";
    fleet = agentOpts "fleet" // {
      roster = lib.mkOption {
        type = lib.types.nullOr (lib.types.listOf (lib.types.attrsOf lib.types.anything));
        default = null;
        description = ''
          The review roster (FleetSession rows, as in .fleet-demo/*-roster.json). Rows carry
          `token_ref` references (`ghapp:<installation>`, `file:<path>`), never secrets.
          With Postgres they are upserted into the roster at every fleet start (rows added
          at runtime are kept); without it they replace /var/lib/agent-seddon/fleet/roster.json.
          `null` leaves the roster to the runtime control plane.
        '';
      };
    };

    clickhouse = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Run a native ClickHouse with the agent schema, per-role passwords and row-level
          security, and point the agents' telemetry/history at it.
        '';
      };
      nativePort = lib.mkOption {
        type = lib.types.port;
        default = 9000;
        description = "ClickHouse native-protocol port (loopback).";
      };
    };

    postgres.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Run a loopback PostgreSQL (`services.agentPostgres`, generated password) and keep
        the transactional stores in it: the fleet roster and post leases, scheduler jobs,
        and the provider, role, forge, GitHub-install and transport cards. Off: the
        roster is a JSON file and the rest stay at their defaults.
      '';
    };

    grpcWeb.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Run the Envoy grpc-web bridge the portal dials.";
    };

    hyperdx = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Run HyperDX as three podman containers (MongoDB, the app, the OTel collector)
          over the managed ClickHouse, and export the agents' traces to it over OTLP. The
          collector accepts OTLP only with the ingestion key HyperDX mints when the first
          account is registered (open `frontendUrl` and sign up);
          `agent-seddon-hyperdx-key.timer` then copies it to the agents and restarts them.
          With `auth = false` there is no account and no key (local app mode).
          Needs `clickhouse.enable`.
        '';
      };
      frontendUrl = lib.mkOption {
        type = lib.types.str;
        default = urlFor hdx.ports.ui;
        defaultText = lib.literalExpression ''"http://''${publicHost}:${toString hdx.ports.ui}"'';
        example = "http://l2:8080";
        description = ''
          The URL browsers open HyperDX at. Its session cookie binds to this host, so it
          must be the name you browse with (a sign-in elsewhere bounces back to login).
          Also the portal's Launch link.
        '';
      };
      auth = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Require a HyperDX login. `false` runs HyperDX's local app mode: no sign-up
          and no login, one built-in team whose sources and connections are seeded from
          the managed ClickHouse at start, and a collector that accepts OTLP without an
          ingestion key (so the key timer is not installed and the agents export with no
          header). Anyone who can reach `frontendUrl` then has full HyperDX access, and
          anyone who can reach the collector can write traces: use it only on a trusted
          network. Sources, saved searches and dashboards made under a signed-up team
          stay in MongoDB but are not shown in local mode.
        '';
      };
      dashboards = lib.mkOption {
        type = lib.types.bool;
        default = !cfg.hyperdx.auth;
        defaultText = lib.literalExpression "!config.services.agentSeddon.hyperdx.auth";
        description = ''
          Provision the agent-seddon dashboards (overview, model calls, tools, review
          fleet) and saved searches into HyperDX, each tagged `agent-seddon`, every time
          the app starts or they change (`agent-seddon-hyperdx-dashboards`). They are
          written through HyperDX's API, which validates them; one you edit in the UI is
          overwritten on the next run, and a same-named dashboard without the tag is
          left alone. Needs `auth = false`: with a login the API wants a session.
        '';
      };
    };

    grafana = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Provision the agent-seddon dashboards (folder "agent-seddon": overview, LLM &
          router, review fleet, tools, sessions/context/memory, gRPC/stores/search,
          traces) into the host's `services.grafana`, with a ClickHouse data source
          (uid `clickhouse`, the read-only `agent_viewer`) and its plugin. The
          dashboards choose their Prometheus data source with a variable, so they use
          the one the host already has. The viewer password reaches Grafana through
          systemd `LoadCredential`, never the store. The ClickHouse plugin is linked into
          Grafana's plugins directory, so plugin preinstall (which provides Prometheus
          on Grafana 13.3+) and plugins installed from the UI keep working; a host that
          sets `services.grafana.declarativePlugins` must list
          `grafanaPlugins.grafana-clickhouse-datasource` there itself. Needs
          `clickhouse.enable`.
        '';
      };
      prometheusUrl = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        example = "http://127.0.0.1:9090";
        description = ''
          Also provision a Prometheus data source (uid `prometheus`) at this URL. Leave
          null when the host's Grafana already has one.
        '';
      };
      adminPasswordFile = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        example = "/run/secrets/grafana-admin";
        description = ''
          A file holding the password for Grafana's `admin` account (absolute, outside
          the store). Grafana reads it through systemd `LoadCredential`, and
          `agent-seddon-grafana-admin` applies it with `grafana cli admin
          reset-admin-password` each time Grafana starts, so the file is the source of
          truth: a password changed in the UI is put back at the next restart. After
          editing the file, `systemctl restart grafana`. Null (the default) leaves the
          password to Grafana. Needs the sqlite database (Grafana's default).
        '';
      };
      generateAdminPassword = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Like `adminPasswordFile`, with a random password generated once at
          `${grafAdminGenDir}/admin.password` (root, 0600); read it with
          `sudo cat`. For a new one, delete the file and `systemctl restart grafana`.
        '';
      };
    };

    dex = {
      enable = lib.mkEnableOption ''
        Dex on loopback as the agent's login provider (GitHub sign-in). The module runs
        Dex and registers the agent as its client; turning the agent's `[auth]` on is the
        host's step (merge `dex.issuerSettings` into `settings.auth.issuers`), since
        without the front door the portal sends no token
      '';
      issuer = lib.mkOption {
        type = lib.types.str;
        default = "";
        example = "https://172.16.50.46:8443/dex";
        description = ''
          Dex's public issuer URL: where browsers reach it (the front door's `/dex`) and
          the `iss` the agent expects. Dex mounts its routes under this path. https, or
          http to 127.0.0.1; no trailing slash.
        '';
      };
      port = lib.mkOption {
        type = lib.types.port;
        default = 5556;
        description = "Dex's loopback port.";
      };
      redirectUris = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        example = [ "https://172.16.50.46:8443/auth/callback" ];
        description = "Where Dex may return a browser to the agent with a code (the agent's `[auth] redirect_uris`).";
      };
      tenant = lib.mkOption {
        type = lib.types.str;
        default = "local";
        description = "The tenant every Dex sign-in maps to (Dex tokens carry none).";
      };
      github = {
        clientId = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          description = "The GitHub OAuth client id Dex signs in with (callback `<issuer>/callback`).";
        };
        clientSecretFile = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          example = "/run/secrets/dex-github";
          description = "A file holding that client's secret (absolute, outside the store).";
        };
        orgs = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ ];
          description = "Only members of these GitHub organizations may finish a sign-in. Empty: any GitHub user (the agent still admits only bound principals).";
        };
      };
      mockLogin = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Tests only: Dex's `mockCallback` connector, which signs anyone in as a fixed user, in place of GitHub.";
      };
      issuerSettings = lib.mkOption {
        type = lib.types.attrsOf lib.types.anything;
        readOnly = true;
        description = "The `[[auth.issuers]]` entry for this Dex, with `backchannel_url` on loopback; merge it into `settings.auth.issuers`.";
      };
    };

    portal = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Serve the portal web bundle on :${toString ports.portalWeb}.";
      };
      defines = lib.mkOption {
        type = lib.types.attrsOf lib.types.str;
        default = { };
        example = {
          PORTAL_GRAFANA_URL = "http://172.16.50.46:3000";
          PORTAL_PROMETHEUS_URL = "http://172.16.50.46:9090";
        };
        description = "Extra `PORTAL_*` compile-time defines for the web bundle.";
      };
    };
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        assertions = [
          {
            assertion = builtins.all (n: builtins.match "[A-Za-z0-9_.-]+" n != null) (
              builtins.attrNames cfg.credentials
            );
            message = "services.agentSeddon.credentials: names must match [A-Za-z0-9_.-]+ (they become paths under /run/credentials).";
          }
          {
            assertion = builtins.all (
              o: builtins.match "https?://[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?(:[0-9]{1,5})?" o != null
            ) cfg.extraOrigins;
            message = "services.agentSeddon.extraOrigins: each must be an exact origin `http(s)://host[:port]` (no wildcard, path or trailing slash).";
          }
          {
            assertion = !hdxEnabled || chEnabled;
            message = "services.agentSeddon.hyperdx needs clickhouse.enable: the collector and the app use the managed ClickHouse.";
          }
          {
            assertion =
              !hdxEnabled
              ||
                builtins.match "https?://[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?(:[0-9]{1,5})?/?" cfg.hyperdx.frontendUrl
                != null
                && portOk cfg.hyperdx.frontendUrl;
            message = "services.agentSeddon.hyperdx.frontendUrl must be `http(s)://host[:port]`.";
          }
          {
            assertion = !(hdxEnabled && cfg.hyperdx.dashboards) || !cfg.hyperdx.auth;
            message = "services.agentSeddon.hyperdx.dashboards needs hyperdx.auth = false: with a login HyperDX's API wants a session.";
          }
          {
            assertion = !cfg.grafana.enable || config.services.grafana.enable;
            message = "services.agentSeddon.grafana provisions into the host's Grafana, but services.grafana.enable is false.";
          }
          {
            assertion =
              cfg.grafana.adminPasswordFile == null
              || (
                lib.hasPrefix "/" cfg.grafana.adminPasswordFile
                && !(lib.hasPrefix builtins.storeDir cfg.grafana.adminPasswordFile)
                && builtins.match ".*[[:space:]].*" cfg.grafana.adminPasswordFile == null
              );
            message = "services.agentSeddon.grafana.adminPasswordFile must be an absolute path outside the Nix store, with no whitespace.";
          }
          {
            assertion = !(cfg.grafana.adminPasswordFile != null && cfg.grafana.generateAdminPassword);
            message = "services.agentSeddon.grafana: set adminPasswordFile or generateAdminPassword, not both.";
          }
          {
            assertion = grafAdminFile == null || cfg.grafana.enable;
            message = "services.agentSeddon.grafana.adminPasswordFile / generateAdminPassword need grafana.enable.";
          }
          {
            assertion =
              grafAdminFile == null || !cfg.grafana.enable || graf.settings.database.type == "sqlite3";
            message = "services.agentSeddon.grafana: the managed admin password needs Grafana's sqlite database (grafana cli cannot read the server's database credential).";
          }
          {
            assertion =
              grafAdminFile == null || !cfg.grafana.enable || graf.settings.server.protocol != "socket";
            message = "services.agentSeddon.grafana: the managed admin password waits on Grafana's HTTP /api/health, so it needs server.protocol http or https, not socket.";
          }
          {
            assertion = !cfg.grafana.enable || chEnabled;
            message = "services.agentSeddon.grafana needs clickhouse.enable: the dashboards read the agent tables and traces from it.";
          }
          {
            assertion =
              cfg.grafana.prometheusUrl == null
              ||
                builtins.match "https?://[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?(:[0-9]{1,5})?/?" cfg.grafana.prometheusUrl
                != null
                && portOk cfg.grafana.prometheusUrl;
            message = "services.agentSeddon.grafana.prometheusUrl must be `http(s)://host[:port]`.";
          }
        ];

        users.users.${cfg.user} = {
          isSystemUser = true;
          group = cfg.group;
          home = stateDir;
          description = "agent-seddon services";
        };
        users.groups.${cfg.group} = { };

        systemd.services = lib.mapAttrs' agentUnit enabledAgents;
        services.agentSeddon.renderedSettings = lib.mapAttrs mergedSettings enabledAgents;

        networking.firewall.allowedTCPPorts = lib.mkIf (cfg.openFirewall && lan) (
          lib.optional cfg.portal.enable ports.portalWeb
          ++ lib.optionals cfg.grpcWeb.enable (
            lib.optional cfg.gateway.enable ports.grpcWeb
            ++ lib.optional cfg.sessions.enable ports.grpcWebSessions
            ++ lib.optional cfg.fleet.enable ports.grpcWebFleet
          )
          ++ lib.optional hdxEnabled hdx.ports.ui
        );
      }

      (lib.mkIf cfg.grpcWeb.enable {
        systemd.services.agent-seddon-grpc-web = {
          description = "agent-seddon Envoy grpc-web bridge";
          wantedBy = [ "multi-user.target" ];
          after = [ "network.target" ];
          serviceConfig = hardening // {
            DynamicUser = true;
            ExecStart = "${seddon.envoy}/bin/envoy -c ${envoyConfig} --disable-hot-restart --log-level warn";
            Restart = "on-failure";
            RestartSec = 5;
          };
        };
      })

      (lib.mkIf cfg.portal.enable {
        systemd.services.agent-seddon-portal = {
          description = "agent-seddon portal (static web bundle)";
          wantedBy = [ "multi-user.target" ];
          after = [ "network.target" ];
          serviceConfig = hardening // {
            DynamicUser = true;
            # --cache-control-headers=false: the bundle's file names are not hashed, so the
            # default one-year cache would pin a stale portal after a redeploy (see portal-web).
            ExecStart = "${seddon.staticWebServer}/bin/static-web-server --root ${webBundle} --host ${bindHost} --port ${toString ports.portalWeb} --cache-control-headers=false";
            Restart = "on-failure";
            RestartSec = 5;
          };
        };
      })

      (lib.mkIf cfg.grafana.enable {
        services.grafana = {
          enable = lib.mkDefault true;
          provision = {
            enable = true;
            dashboards.settings.providers = [
              {
                name = "agent-seddon";
                folder = "agent-seddon";
                type = "file";
                # Store-path dashboards: an edit in the UI could not be saved back.
                allowUiUpdates = false;
                options.path = seddon.grafanaDashboards;
              }
            ];
            datasources.settings.datasources = [
              {
                name = "agent-seddon ClickHouse";
                uid = "clickhouse";
                type = "grafana-clickhouse-datasource";
                jsonData = {
                  host = "127.0.0.1";
                  port = cfg.clickhouse.nativePort;
                  protocol = "native";
                  secure = false;
                  username = "agent_viewer";
                  defaultDatabase = "agent";
                };
                # Grafana reads the file at start; LoadCredential below puts it there.
                secureJsonData.password = "$__file{/run/credentials/grafana.service/ch-viewer}";
              }
            ]
            ++ lib.optional (cfg.grafana.prometheusUrl != null) {
              name = "agent-seddon Prometheus";
              uid = "prometheus";
              type = "prometheus";
              url = cfg.grafana.prometheusUrl;
            };
          };
        };

        # Linked, not `declarativePlugins`: that would turn off plugin preinstall, which is
        # where Grafana 13.3+ gets its Prometheus data source.
        systemd.tmpfiles.rules = [
          "L+ ${config.services.grafana.settings.paths.plugins}/grafana-clickhouse-datasource - - - - ${pkgs.grafanaPlugins.grafana-clickhouse-datasource}"
        ];

        # The viewer password is generated on first start (0700, the agent user);
        # LoadCredential copies it to a file only the grafana unit can read.
        systemd.services.grafana = {
          after = [ "agent-seddon-clickhouse-creds.service" ];
          # Like postgresql-setup: without the password file LoadCredential fails anyway.
          requires = [ "agent-seddon-clickhouse-creds.service" ];
          serviceConfig.LoadCredential = [ "ch-viewer:${chCredsDir}/viewer.password" ];
        };
      })

      (lib.mkIf (cfg.grafana.enable && grafAdminFile != null) {
        # Used when Grafana first creates its database; afterwards the reset below.
        services.grafana.settings.security.admin_password =
          "$__file{/run/credentials/grafana.service/admin-password}";
        systemd.services.grafana = {
          after = lib.optional cfg.grafana.generateAdminPassword "agent-seddon-grafana-admin-password.service";
          requires = lib.optional cfg.grafana.generateAdminPassword "agent-seddon-grafana-admin-password.service";
          serviceConfig.LoadCredential = [ "admin-password:${grafAdminFile}" ];
        };

        # Grafana reads admin_password only when it creates its database, so the file is
        # applied on every start. The CLI reads the password from stdin: never argv.
        systemd.services.agent-seddon-grafana-admin = {
          description = "agent-seddon Grafana admin password";
          after = [ "grafana.service" ];
          partOf = [ "grafana.service" ];
          wantedBy = [ "grafana.service" ];
          path = [ pkgs.curl ];
          script = ''
            # Until the server has migrated the database: up to 5 minutes here, then
            # Restart below. Loopback health only, so -k is harmless for an https
            # Grafana with a host-name cert.
            i=0
            until curl -fsSk --max-time 5 -o /dev/null \
              "${graf.settings.server.protocol}://${grafLocalAddr}:${toString graf.settings.server.http_port}/api/health"; do
              i=$((i + 1))
              if [ "$i" -ge 60 ]; then
                echo "agent-seddon: Grafana is not healthy after 5 minutes" >&2
                exit 1
              fi
              sleep 5
            done
            # The server's own homepath: nixpkgs links the package's conf/ into dataDir.
            ${lib.getExe graf.package} cli --homepath ${graf.dataDir} --config ${grafCliIni} \
              admin reset-admin-password --password-from-stdin \
              < "$CREDENTIALS_DIRECTORY/admin-password" >/dev/null
            echo "agent-seddon: Grafana admin password applied"
          '';
          unitConfig = {
            StartLimitIntervalSec = 600;
            StartLimitBurst = 30;
          };
          serviceConfig = hardening // {
            Type = "oneshot";
            RemainAfterExit = true;
            User = "grafana";
            Group = "grafana";
            LoadCredential = [ "admin-password:${grafAdminFile}" ];
            ReadWritePaths = [ graf.dataDir ];
            # Grafana's own: the database stays group-readable as the server leaves it.
            UMask = "0027";
            Restart = "on-failure";
            RestartSec = "5s";
          };
        };
      })

      (lib.mkIf (cfg.grafana.enable && cfg.grafana.generateAdminPassword) {
        # Once: an existing file is never replaced.
        systemd.services.agent-seddon-grafana-admin-password = {
          description = "agent-seddon Grafana admin password (generate once)";
          before = [ "grafana.service" ];
          # Restarting Grafana re-runs this, so a deleted file is made again.
          partOf = [ "grafana.service" ];
          path = [ pkgs.coreutils ];
          script = ''
            f=${grafAdminGenDir}/admin.password
            if [ ! -s "$f" ]; then
              umask 077
              head -c 24 /dev/urandom | base64 | tr -d '/+=\n' > "$f.tmp"
              mv "$f.tmp" "$f"
            fi
          '';
          serviceConfig = hardening // {
            Type = "oneshot";
            RemainAfterExit = true;
            StateDirectory = "agent-seddon-grafana";
            StateDirectoryMode = "0700";
          };
        };
      })

      {
        services.agentSeddon.dex.issuerSettings = {
          name = "dex";
          profile = "generic";
          inherit (dexCfg) issuer;
          audience = dexClientId;
          jwks_url = "${dexLocal}/keys";
          backchannel_url = dexLocal;
          default_tenant = dexCfg.tenant;
          require_email_verified = true;
          client_secret = "file:/run/credentials/@unit@/dex-client";
        };
      }

      (lib.mkIf dexCfg.enable {
        assertions = [
          {
            assertion =
              builtins.match "https://[A-Za-z0-9.-]+(:[0-9]{1,5})?(/[A-Za-z0-9._~-]+)*" dexCfg.issuer != null
              || builtins.match "http://127\\.0\\.0\\.1(:[0-9]{1,5})?(/[A-Za-z0-9._~-]+)*" dexCfg.issuer != null;
            message = "services.agentSeddon.dex.issuer must be https://host[:port][/path] (or http://127.0.0.1…), with no trailing slash, query or fragment.";
          }
          {
            assertion = dexCfg.mockLogin != (dexCfg.github.clientId != null);
            message = "services.agentSeddon.dex: set github.clientId (with github.clientSecretFile), or mockLogin for a test, not both.";
          }
          {
            assertion = (dexCfg.github.clientId == null) == (dexCfg.github.clientSecretFile == null);
            message = "services.agentSeddon.dex.github: clientId and clientSecretFile go together.";
          }
          {
            assertion =
              dexCfg.github.clientSecretFile == null
              || (
                lib.hasPrefix "/" dexCfg.github.clientSecretFile
                && !(lib.hasPrefix builtins.storeDir dexCfg.github.clientSecretFile)
                && builtins.match ".*[[:space:]'\\].*" dexCfg.github.clientSecretFile == null
              );
            message = "services.agentSeddon.dex.github.clientSecretFile must be an absolute path outside the Nix store, with no whitespace, quote or backslash.";
          }
          {
            assertion = dexCfg.redirectUris != [ ];
            message = "services.agentSeddon.dex.redirectUris: list where Dex may return a browser to the agent.";
          }
          {
            assertion = builtins.all (
              o: builtins.match "[A-Za-z0-9][A-Za-z0-9-]*" o != null
            ) dexCfg.github.orgs;
            message = "services.agentSeddon.dex.github.orgs: GitHub organization names only.";
          }
        ];

        services.agentSeddon.credentials.dex-client = dexClientSecret;

        services.dex = {
          enable = true;
          settings = {
            inherit (dexCfg) issuer;
            storage = {
              type = "sqlite3";
              config.file = "/var/lib/dex/dex.db";
            };
            web.http = "127.0.0.1:${toString dexCfg.port}";
            oauth2.skipApprovalScreen = true;
            staticClients = [
              {
                id = dexClientId;
                name = "agent-seddon";
                redirectURIs = dexCfg.redirectUris;
                secretFile = dexClientSecret;
              }
            ];
            connectors =
              if dexCfg.mockLogin then
                [
                  {
                    type = "mockCallback";
                    id = "mock";
                    name = "Mock";
                  }
                ]
              else
                [
                  {
                    type = "github";
                    id = "github";
                    name = "GitHub";
                    config = {
                      clientID = dexCfg.github.clientId;
                      # Replaced with the file's contents at start (below), like secretFile.
                      clientSecret = dexCfg.github.clientSecretFile;
                      redirectURI = "${dexCfg.issuer}/callback";
                    }
                    // lib.optionalAttrs (dexCfg.github.orgs != [ ]) {
                      orgs = map (name: { inherit name; }) dexCfg.github.orgs;
                    };
                  }
                ];
          };
        };

        systemd.services = {
          dex = {
            after = [ "agent-seddon-dex-secret.service" ];
            requires = [ "agent-seddon-dex-secret.service" ];
            serviceConfig = {
              StateDirectory = "dex";
              # Root, like the nixpkgs module's own secretFile step.
              ExecStartPre = lib.mkIf (dexCfg.github.clientSecretFile != null) (
                lib.mkAfter [
                  "+${pkgs.replace-secret}/bin/replace-secret ${dexCfg.github.clientSecretFile} ${dexCfg.github.clientSecretFile} /run/dex/config.yaml"
                ]
              );
            };
          };
          # Once: an existing secret is never replaced. Hex, so it is safe inside YAML.
          agent-seddon-dex-secret = {
            description = "agent-seddon client secret at Dex (generate once)";
            before = [ "dex.service" ] ++ map (n: "${unitName n}.service") (lib.attrNames enabledAgents);
            path = [ pkgs.coreutils ];
            script = ''
              f=${dexClientSecret}
              [ -s "$f" ] && exit 0
              umask 077
              od -An -N32 -tx1 /dev/urandom | tr -d ' \n' > "$f.tmp"
              mv "$f.tmp" "$f"
            '';
            serviceConfig = hardening // {
              Type = "oneshot";
              RemainAfterExit = true;
              StateDirectory = "agent-seddon-dex";
              StateDirectoryMode = "0700";
            };
          };
        }
        // lib.mapAttrs' (
          n: _:
          lib.nameValuePair (unitName n) {
            after = [ "agent-seddon-dex-secret.service" ];
            requires = [ "agent-seddon-dex-secret.service" ];
          }
        ) enabledAgents;
      })

      (lib.mkIf pgEnabled {
        services.agentPostgres = {
          enable = true;
          # Copied in by LoadCredential below: postgresql-setup runs as `postgres`, which
          # cannot read the agent user's 0700 directory.
          passwordFile = lib.mkDefault "/run/credentials/postgresql-setup.service/agent-pg-password";
        };

        systemd.services.postgresql-setup = {
          after = [ "agent-seddon-postgres-creds.service" ];
          requires = [ "agent-seddon-postgres-creds.service" ];
          serviceConfig.LoadCredential = [ "agent-pg-password:${pgCredsDir}/password" ];
        };

        # Generate the role password once (never rotated) and the DSN the agents read
        # through `[config_store] dsn_ref = "file:…"`. Hex, so it needs no URL escaping.
        systemd.services.agent-seddon-postgres-creds = {
          description = "agent-seddon PostgreSQL credentials";
          before = [ "postgresql-setup.service" ];
          path = [ pkgs.coreutils ];
          script = ''
            cd ${pgCredsDir}
            if [ ! -s password ]; then
              od -An -tx1 -N24 /dev/urandom | tr -d ' \n' > password.tmp
              mv password.tmp password
            fi
            printf 'postgres://%s:%s@127.0.0.1:%s/%s' ${pg.user} "$(cat password)" \
              ${toString pg.port} ${pg.database} > dsn.tmp
            mv dsn.tmp dsn
          '';
          serviceConfig = hardening // {
            Type = "oneshot";
            RemainAfterExit = true;
            User = cfg.user;
            Group = cfg.group;
            StateDirectory = "agent-seddon/postgres";
            StateDirectoryMode = "0700";
          };
        };
      })

      (lib.mkIf chEnabled {
        services.clickhouse = {
          enable = true;
          serverConfig = {
            # nix/clickhouse/config.xml: the reader's row policies bind `SQL_tenant_id`,
            # and a user with no matching policy sees no rows.
            custom_settings_prefixes = "SQL_";
            access_control_improvements.users_without_row_policies_can_read_rows = false;
            listen_host = "127.0.0.1";
            tcp_port = cfg.clickhouse.nativePort;
          };
          # The admin (`default`) gets the generated password — its SHA-256 comes from the
          # environment so it never enters the store — and loopback-only access.
          usersConfig.users.default = {
            password = {
              "@remove" = "1";
            };
            password_sha256_hex = {
              "@from_env" = "AGENT_CH_ADMIN_SHA256";
            };
            networks = {
              "@replace" = "replace";
              ip = [
                "127.0.0.1"
                "::1"
              ];
            };
            access_management = 1;
          };
        };

        systemd.services.clickhouse = {
          after = [ "agent-seddon-clickhouse-creds.service" ];
          requires = [ "agent-seddon-clickhouse-creds.service" ];
          serviceConfig.EnvironmentFile = "${chCredsDir}/admin.env";
        };

        # Generate the per-role passwords once (ch_creds.py `ensure`: never rotates an
        # existing file) and the admin hash env file ClickHouse reads.
        systemd.services.agent-seddon-clickhouse-creds = {
          description = "agent-seddon ClickHouse credentials";
          before = [ "clickhouse.service" ];
          environment.XDG_STATE_HOME = "/var/lib";
          path = [
            seddon.chCreds
            pkgs.coreutils
          ];
          script = ''
            clickhouse-creds ensure >/dev/null
            sha="$(printf '%s' "$(tr -d '\n' < ${chCredsDir}/admin.password)" | sha256sum | cut -d' ' -f1)"
            printf 'AGENT_CH_ADMIN_SHA256=%s\n' "$sha" > ${chCredsDir}/admin.env.tmp
            mv ${chCredsDir}/admin.env.tmp ${chCredsDir}/admin.env
          '';
          serviceConfig = hardening // {
            Type = "oneshot";
            RemainAfterExit = true;
            User = cfg.user;
            Group = cfg.group;
            # Only the clickhouse/ subdirectory, 0700: StateDirectoryMode applies to every
            # listed dir, and ch_creds.py refuses a group-readable one. The agent units
            # (same user) own the 0750 parent.
            StateDirectory = "agent-seddon/clickhouse";
            StateDirectoryMode = "0700";
          };
        };

        # Apply the idempotent schema + role passwords on every boot/upgrade (the
        # `clickhouse-migrate` step a manual redeploy once forgot — fleet-redeploy.nix).
        systemd.services.agent-seddon-clickhouse-schema = {
          description = "agent-seddon ClickHouse schema";
          after = [ "clickhouse.service" ];
          requires = [ "clickhouse.service" ];
          wantedBy = [ "multi-user.target" ];
          environment.XDG_STATE_HOME = "/var/lib";
          path = [
            seddon.chCreds
            config.services.clickhouse.package
            pkgs.coreutils
          ];
          script = ''
            CLICKHOUSE_PASSWORD="$(tr -d '\n' < ${chCredsDir}/admin.password)"
            export CLICKHOUSE_PASSWORD
            ch() { clickhouse-client --host 127.0.0.1 --port ${toString cfg.clickhouse.nativePort} "$@"; }
            for _ in $(seq 60); do ch --query 'SELECT 1' >/dev/null 2>&1 && break; sleep 1; done
            ch --multiquery < ${seddon.clickhouseSchema}
            clickhouse-creds alter-sql | ch --multiquery
          '';
          serviceConfig = hardening // {
            Type = "oneshot";
            RemainAfterExit = true;
            User = cfg.user;
            Group = cfg.group;
          };
        };
      })

      (lib.mkIf hdxEnabled (
        let
          app = hdx.names.app;
          otel = hdx.names.otel;
          mongo = hdx.names.mongo;
          podmanUnit = n: "podman-${n}.service";
          agentUnits = map (n: "${unitName n}.service") (lib.attrNames enabledAgents);
        in
        {
          # Host networking: only Mongo binds loopback; the collector (4317/4318) and the
          # app's API and OpAMP (8000/4320) listen on every address whatever publicHost
          # says, and only the firewall keeps them off the network.
          warnings =
            lib.optional (!config.networking.firewall.enable) ''
              services.agentSeddon.hyperdx: the firewall is off, so HyperDX's OTLP (4317/4318),
              API (8000) and OpAMP (4320) ports are reachable from the network. OpAMP never
              asks for a credential. ${
                if hdxAuth then
                  ''OTLP needs the ingestion key only when the team's "collector authentication" setting is on.''
                else
                  "With hyperdx.auth = false neither OTLP nor the API asks for one either, and the UI (${toString hdx.ports.ui}) is open with no login."
              }
            ''
            # The likelier exposure: the UI is opened to the LAN on purpose.
            ++ lib.optional (!hdxAuth && cfg.openFirewall && lan && config.networking.firewall.enable) ''
              services.agentSeddon.hyperdx: auth = false and openFirewall open the HyperDX UI
              (${toString hdx.ports.ui}) to the network with no login; anyone who reaches
              ${cfg.hyperdx.frontendUrl} has full HyperDX access.
            '';

          virtualisation.oci-containers.containers = {
            ${mongo} = {
              image = hdx.mongoImage;
              # The image's entrypoint binds every address unless told otherwise.
              cmd = [
                "--bind_ip"
                "127.0.0.1"
              ];
              volumes = [ "/var/lib/agent-seddon-hyperdx/mongo:/data/db" ];
              extraOptions = [ "--network=host" ];
            };
            ${app} = {
              image = hdx.appImage;
              dependsOn = [ mongo ];
              environment = {
                MONGO_URI = "mongodb://127.0.0.1:${toString hdx.ports.mongo}/hyperdx";
                FRONTEND_URL = cfg.hyperdx.frontendUrl;
                HYPERDX_APP_URL = "http://localhost";
                HYPERDX_APP_PORT = toString hdx.ports.ui;
                HYPERDX_API_PORT = toString hdx.ports.api;
                OPAMP_PORT = toString hdx.ports.opamp;
                OTEL_EXPORTER_OTLP_ENDPOINT = "http://127.0.0.1:${toString hdx.ports.otlpHttp}";
                OTEL_SERVICE_NAME = "hdx-oss-app";
                USAGE_STATS_ENABLED = "false";
                DEFAULT_SOURCES = hdx.defaultSources;
              }
              # HyperDX's switch for local app mode; the value is its own guard string
              # (packages/api config.ts: IS_LOCAL_APP_MODE === 'DANGEROUSLY_is_local_app_mode💀').
              // lib.optionalAttrs (!hdxAuth) { IS_LOCAL_APP_MODE = "DANGEROUSLY_is_local_app_mode💀"; };
              # DEFAULT_CONNECTIONS carries the agent_viewer password.
              environmentFiles = [ "${hdxDir}/app.env" ];
              extraOptions = [ "--network=host" ];
            }
            # The image's /etc/local/entry.sh overwrites IS_LOCAL_APP_MODE with
            # "REQUIRED_AUTH" before starting the API, so run a copy without that line.
            # If the image stops setting it that exact way, refuse to start rather than
            # silently keep the login.
            // lib.optionalAttrs (!hdxAuth) {
              entrypoint = "/bin/sh";
              cmd = [
                "-c"
                hdxLocalEntry
              ];
            };
            ${otel} = {
              image = hdx.otelImage;
              dependsOn = [ app ];
              environment = {
                CLICKHOUSE_ENDPOINT = "tcp://127.0.0.1:${toString cfg.clickhouse.nativePort}?dial_timeout=10s";
                CLICKHOUSE_USER = "default";
                HYPERDX_OTEL_EXPORTER_CLICKHOUSE_DATABASE = hdx.otelDb;
                # The MergeTree schema, not the JSON-typed one.
                HYPERDX_OTEL_EXPORTER_CREATE_LEGACY_SCHEMA = "true";
                OPAMP_SERVER_URL = "http://127.0.0.1:${toString hdx.ports.opamp}";
                HYPERDX_LOG_LEVEL = "info";
              };
              # CLICKHOUSE_PASSWORD: the admin's, since the collector creates otel_*.
              environmentFiles = [ "${hdxDir}/otel.env" ];
              extraOptions = [ "--network=host" ];
            };
          };

          systemd.tmpfiles.rules = [ "d /var/lib/agent-seddon-hyperdx/mongo 0700 root root -" ];

          systemd.services = {
            "podman-${app}" = {
              after = [
                "agent-seddon-hyperdx-env.service"
                "agent-seddon-clickhouse-schema.service"
              ];
              requires = [
                "agent-seddon-hyperdx-env.service"
                "agent-seddon-clickhouse-schema.service"
              ];
            };
            "podman-${otel}" = {
              after = [
                "agent-seddon-hyperdx-env.service"
                "agent-seddon-clickhouse-schema.service"
              ];
              requires = [
                "agent-seddon-hyperdx-env.service"
                "agent-seddon-clickhouse-schema.service"
              ];
            };

            # Render the containers' secret environment from the ClickHouse passwords,
            # into the agent user's 0700 directory (podman reads it as root).
            agent-seddon-hyperdx-env = {
              description = "agent-seddon HyperDX container environment";
              after = [ "agent-seddon-clickhouse-creds.service" ];
              requires = [ "agent-seddon-clickhouse-creds.service" ];
              environment.XDG_STATE_HOME = "/var/lib";
              path = [
                seddon.chCreds
                pkgs.coreutils
              ];
              script = ''
                cd ${hdxDir}
                printf 'DEFAULT_CONNECTIONS=%s\n' \
                  "$(clickhouse-creds hyperdx-connections --url http://127.0.0.1:8123)" > app.env.tmp
                printf 'CLICKHOUSE_PASSWORD=%s\n' "$(tr -d '\n' < ${chCredsDir}/admin.password)" > otel.env.tmp
                mv app.env.tmp app.env
                mv otel.env.tmp otel.env
              '';
              serviceConfig = hardening // {
                Type = "oneshot";
                RemainAfterExit = true;
                User = cfg.user;
                Group = cfg.group;
                StateDirectory = "agent-seddon/hyperdx";
                StateDirectoryMode = "0700";
              };
            };

            # Copy the ingestion key HyperDX minted at sign-up to the agents (as the OTLP
            # `authorization` header). As the agent user, over Mongo's loopback port, with
            # the same sandbox as the agents. Quiet until the account exists.
            agent-seddon-hyperdx-key = lib.mkIf hdxAuth {
              description = "agent-seddon HyperDX ingestion key for the agents";
              wants = [ "agent-seddon-hyperdx-env.service" ];
              after = [
                "agent-seddon-hyperdx-env.service"
                (podmanUnit mongo)
              ];
              path = [
                pkgs.mongosh
                pkgs.coreutils
              ];
              environment.HOME = hdxDir;
              script = ''
                if ! key="$(mongosh --quiet --norc mongodb://127.0.0.1:${toString hdx.ports.mongo}/hyperdx --eval \
                  'const t = db.teams.findOne(); print(t && t.apiKey ? t.apiKey : "")' 2>/tmp/err)"; then
                  echo "agent-seddon-hyperdx-key: Mongo not reachable yet: $(head -c 200 /tmp/err)" >&2
                  exit 0
                fi
                key="$(printf '%s' "$key" | tr -d '[:space:]')"
                [ -n "$key" ] || exit 0
                # Header-safe whatever HyperDX's format: no separators, bounded.
                if ! [[ "$key" =~ ^[A-Za-z0-9._-]{8,128}$ ]]; then
                  echo "agent-seddon-hyperdx-key: the team's apiKey is not a plain token; not using it" >&2
                  exit 0
                fi
                want="authorization=$key"
                if [ -f ${otlpHeadersFile} ] && [ "$(cat ${otlpHeadersFile})" = "$want" ]; then exit 0; fi
                printf '%s' "$want" > ${otlpHeadersFile}.tmp
                mv ${otlpHeadersFile}.tmp ${otlpHeadersFile}
                echo "agent-seddon-hyperdx-key: ingestion key updated"
              '';
              serviceConfig = hardening // {
                Type = "oneshot";
                User = cfg.user;
                Group = cfg.group;
                ReadWritePaths = [ hdxDir ];
              };
            };
          }
          # A new key file restarts the agents (they read it at start). Root, to reach
          # PID 1; it runs nothing else.
          # The agent-seddon dashboards and saved searches, through the app's API. Runs
          # each time the app starts (a wiped Mongo reseeds the sources with new ids) and
          # when the template changes; retries until the API answers and the sources exist.
          // lib.optionalAttrs cfg.hyperdx.dashboards {
            agent-seddon-hyperdx-dashboards = {
              description = "agent-seddon HyperDX dashboards and saved searches";
              wantedBy = [
                "multi-user.target"
                (podmanUnit app)
              ];
              after = [ (podmanUnit app) ];
              # A oneshot that stays active is not re-run by `wants`; PartOf makes an app
              # restart (say, after a wiped Mongo reseeded the sources with new ids)
              # restart it too.
              partOf = [ (podmanUnit app) ];
              restartTriggers = [ hdx.dashboardsTemplate ];
              serviceConfig = hardening // {
                Type = "oneshot";
                RemainAfterExit = true;
                User = cfg.user;
                Group = cfg.group;
                ExecStart = "${seddon.hyperdxProvision} --api http://127.0.0.1:${toString hdx.ports.api} --template ${hdx.dashboardsTemplate}";
                Restart = "on-failure";
                RestartSec = "1min";
              };
            };
          }
          // lib.optionalAttrs (hdxAuth && agentUnits != [ ]) {
            agent-seddon-hyperdx-restart = {
              description = "Restart the agents for a new HyperDX ingestion key";
              serviceConfig = hardening // {
                Type = "oneshot";
                ExecStart = "${pkgs.systemd}/bin/systemctl try-restart ${lib.concatStringsSep " " agentUnits}";
              };
            };
          };

          systemd.paths = lib.mkIf (hdxAuth && agentUnits != [ ]) {
            agent-seddon-hyperdx-restart = {
              wantedBy = [ "multi-user.target" ];
              pathConfig.PathChanged = otlpHeadersFile;
            };
          };

          systemd.timers.agent-seddon-hyperdx-key = lib.mkIf hdxAuth {
            description = "Copy the HyperDX ingestion key to the agents";
            wantedBy = [ "timers.target" ];
            timerConfig = {
              OnBootSec = "2min";
              OnUnitActiveSec = "2min";
            };
          };
        }
      ))
    ]
  );
}
