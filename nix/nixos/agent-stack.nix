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
  configFile =
    name: svc:
    let
      merged = withProviderKey (
        lib.foldl' lib.recursiveUpdate repoConfig [
          baseSettings
          svc.extra
          cfg.settings
          cfg.${name}.settings
        ]
      );
      raw = toml.generate "agent-seddon-${name}.toml" merged;
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
        ];

        users.users.${cfg.user} = {
          isSystemUser = true;
          group = cfg.group;
          home = stateDir;
          description = "agent-seddon services";
        };
        users.groups.${cfg.group} = { };

        systemd.services = lib.mapAttrs' agentUnit enabledAgents;

        networking.firewall.allowedTCPPorts = lib.mkIf (cfg.openFirewall && lan) (
          lib.optional cfg.portal.enable ports.portalWeb
          ++ lib.optionals cfg.grpcWeb.enable (
            lib.optional cfg.gateway.enable ports.grpcWeb
            ++ lib.optional cfg.sessions.enable ports.grpcWebSessions
            ++ lib.optional cfg.fleet.enable ports.grpcWebFleet
          )
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
    ]
  );
}
