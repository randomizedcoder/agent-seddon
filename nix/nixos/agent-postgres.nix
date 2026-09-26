# nix/nixos/agent-postgres.nix
#
# NixOS-native production deployment of the transactional config store
# (`agent-config-store` postgres tier, config C41 / A2). This is the PRODUCTION
# counterpart to the opt-in `postgres-*` container apps (nix/postgres/): where the
# container is the quick local/CI spin, this module runs the server as a
# first-class `services.postgresql` service on a NixOS host (the l2 box), with a
# persistent data directory, tuning knobs, a secret password read from a file
# (never inlined into the store), and a loopback-by-default bind surface.
#
# Exposed from agent-seddon's own flake as `nixosModules.agent-postgres`, so an
# external host config (e.g. `~/nixos/desktop/l2`, a SEPARATE repo) imports it:
#
#   # flake.nix of the host repo
#   inputs.agent-seddon.url = "github:randomizedcoder/agent-seddon";
#   # in the nixosSystem modules list:
#   agent-seddon.nixosModules.agent-postgres
#   { services.agentPostgres = {
#       enable = true;
#       passwordFile = "/run/secrets/agent-pg-password";   # e.g. an agenix/sops secret
#     }; }
#
# The agent then dials it with `AGENT_CONFIG_STORE_DSN` pointing at
# `postgres://<user>:<pw>@127.0.0.1:5432/<database>` (the DSN password itself comes
# from `env:`/`file:` per the config-store secret rules — never inlined either).
#
# UNTRUSTED-INPUT NOTE: the operator owns every value here, but the password from
# `passwordFile` is still applied to `ALTER ROLE` via psql's `:'var'` quoting form
# (which escapes it safely) rather than string interpolation, so a password
# containing quotes can't break — or inject — the statement.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.agentPostgres;

  # LAN-bound iff any listen address is not a loopback address. Drives whether the
  # firewall port is opened and whether the trusted-CIDR hba lines are needed.
  isLanBound = lib.any (a: a != "127.0.0.1" && a != "::1" && a != "localhost") cfg.listenAddresses;
in
{
  options.services.agentPostgres = {
    enable = lib.mkEnableOption "the agent-seddon PostgreSQL config store (agent-config-store postgres tier)";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.postgresql_16;
      defaultText = lib.literalExpression "pkgs.postgresql_16";
      description = ''
        The PostgreSQL server package. Pinned to 16 to match the container image
        (`nix/versions.nix` `postgresImage = "postgres:16"`) and the dev-shell
        `psql`, so the whole track speaks one major.
      '';
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 5432;
      description = "TCP port the server listens on.";
    };

    dataDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/postgresql/16";
      description = ''
        Persistent data directory. Survives service restarts and system upgrades;
        this is where the config-store `cards`/`tenants` tables live. The role
        password is fixed at first initialisation of an empty dir (standard
        PostgreSQL behaviour) but this module RE-APPLIES it from `passwordFile` on
        every start (see `postStart`), so rotating the secret takes effect on
        the next restart without wiping the data.
      '';
    };

    database = lib.mkOption {
      type = lib.types.str;
      default = "agent_config";
      description = "Database the agent config store uses (mirrors `postgresDatabase`).";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "agent";
      description = "Role the agent connects as; owns `database` (mirrors `postgresUser`).";
    };

    passwordFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/run/secrets/agent-pg-password";
      description = ''
        Path to a file containing the role's password (e.g. an agenix/sops-nix
        secret). REQUIRED when `enable = true`. The password is NEVER inlined into
        the Nix store — only this path is; the file is read at service start.
      '';
    };

    listenAddresses = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "127.0.0.1" ];
      example = [
        "127.0.0.1"
        "172.16.50.46"
      ];
      description = ''
        Addresses the server binds. Loopback-only by default (D5): the agent/fleet
        runs co-located with the DB on the l2 box, so nothing needs a LAN bind. Add
        a LAN address only if the agent runs on a different host — doing so opens
        the firewall port and requires `trustedCidrs` to admit the client.
      '';
    };

    trustedCidrs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "172.16.50.0/24" ];
      description = ''
        Extra CIDRs granted `scram-sha-256` `host` access to `database` as `user`,
        in addition to the always-present loopback lines. Only meaningful when
        `listenAddresses` includes a LAN address.
      '';
    };

    settings = {
      sharedBuffers = lib.mkOption {
        type = lib.types.str;
        default = "256MB";
        description = "`shared_buffers` (mirrors `postgresSharedBuffers`).";
      };
      maxConnections = lib.mkOption {
        type = lib.types.ints.positive;
        default = 100;
        description = ''
          `max_connections`. Keep ≥ the sum of every dialing process's
          `[config_store] pool_max` (mirrors `postgresMaxConnections`).
        '';
      };
      workMem = lib.mkOption {
        type = lib.types.str;
        default = "16MB";
        description = "`work_mem` (mirrors `postgresWorkMem`).";
      };
      effectiveCacheSize = lib.mkOption {
        type = lib.types.str;
        default = "1GB";
        description = "`effective_cache_size` (mirrors `postgresEffectiveCacheSize`).";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.passwordFile != null;
        message = "services.agentPostgres.passwordFile must be set (a secret file path) when the service is enabled — the password is never inlined into the Nix store.";
      }
    ];

    services.postgresql = {
      enable = true;
      package = cfg.package;
      dataDir = cfg.dataDir;

      ensureDatabases = [ cfg.database ];
      ensureUsers = [
        {
          name = cfg.user;
          ensureDBOwnership = true;
        }
      ];

      settings = {
        port = cfg.port;
        listen_addresses = lib.concatStringsSep "," cfg.listenAddresses;
        shared_buffers = cfg.settings.sharedBuffers;
        max_connections = cfg.settings.maxConnections;
        work_mem = cfg.settings.workMem;
        effective_cache_size = cfg.settings.effectiveCacheSize;
        # Store password hashes as SCRAM (the modern default); the hba lines below
        # require it for TCP clients.
        password_encryption = "scram-sha-256";
      };

      # pg_hba, per bind (D5). Loopback always admits `user`→`database` via SCRAM
      # (the agent's DSN carries a password); each `trustedCidrs` entry extends that
      # to a LAN client. Local (unix-socket) peer auth is kept for admin/`postStart`.
      authentication = lib.mkOverride 10 (
        lib.concatStringsSep "\n" (
          [
            "# Managed by services.agentPostgres — do not edit by hand."
            "local   all             all                                     peer"
            "host    ${cfg.database}  ${cfg.user}      127.0.0.1/32            scram-sha-256"
            "host    ${cfg.database}  ${cfg.user}      ::1/128                 scram-sha-256"
          ]
          ++ map (
            cidr: "host    ${cfg.database}  ${cfg.user}      ${cidr}            scram-sha-256"
          ) cfg.trustedCidrs
        )
      );
    };

    # Apply the role password from the secret file on every start. `ensureUsers`
    # creates the role (no password / peer only); this sets its SCRAM password so
    # TCP clients can authenticate. psql's `:'pw'` form quotes+escapes the value, so
    # a password containing quotes can neither break nor inject the statement.
    systemd.services.postgresql.postStart = lib.mkAfter ''
      set -euo pipefail
      pw="$(cat ${lib.escapeShellArg (toString cfg.passwordFile)})"
      ${cfg.package}/bin/psql --port=${toString cfg.port} -v ON_ERROR_STOP=1 \
        --set=pw="$pw" \
        -c 'ALTER ROLE ${cfg.user} WITH PASSWORD :'"'"'pw'"'"';'
    '';

    # Open the firewall ONLY when actually LAN-bound — loopback-only needs nothing.
    networking.firewall.allowedTCPPorts = lib.mkIf isLanBound [ cfg.port ];
  };
}
