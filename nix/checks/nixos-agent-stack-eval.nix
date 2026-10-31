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
  };

  failed = lib.filterAttrs (_: v: !v) checks;
in
pkgs.runCommand "nixos-agent-stack-eval" { } (
  if failed == { } then
    ''
      echo "nixosModules.agent-stack evaluates: ${toString (lib.length (lib.attrNames checks))} checks (units, user, credentials, clickhouse, postgres, firewall loopback+LAN)" > "$out"
    ''
  else
    throw "nixos-agent-stack-eval: unmet assertions: ${lib.concatStringsSep ", " (lib.attrNames failed)}"
)
