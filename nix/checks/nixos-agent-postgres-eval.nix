# nix/checks/nixos-agent-postgres-eval.nix
#
# Hermetic EVAL-ONLY gate for the NixOS-native config-store module
# (`nixosModules.agent-postgres`, PG-05). It instantiates a throwaway NixOS
# configuration that imports the module with `enable = true` and asserts the module
# wires `services.postgresql` the way it promises — enabled, the config-store
# database + owning role ensured, and (with the default loopback bind) the firewall
# left closed.
#
# This is PURE EVALUATION: it reads a handful of resolved option values and never
# builds `system.build.toplevel`, so it needs neither KVM nor a VM (a full
# `nixosTest` boot is not gate-safe in the sandbox — see the plan). It is the
# NixOS twin of the `config-roundtrip` check: prove the deployment surface resolves
# to the intended impls, offline.
{
  pkgs,
}:

let
  lib = pkgs.lib;
  system = pkgs.stdenv.hostPlatform.system;

  # Evaluate the module inside a minimal NixOS configuration. `eval-config.nix`
  # imports its own nixpkgs for `system`; we only read leaf options, so the eval
  # stays lazy (no toplevel, no derivations forced).
  evaluated = import "${pkgs.path}/nixos/lib/eval-config.nix" {
    inherit system;
    modules = [
      ../nixos/agent-postgres.nix
      {
        services.agentPostgres = {
          enable = true;
          # A store PATH stands in for the runtime secret file — the module only
          # records the path (it reads the file at service start, not at eval).
          passwordFile = "/run/secrets/agent-pg-password";
        };
        # Silence the stateVersion nag without pulling in host specifics.
        system.stateVersion = "24.11";
      }
    ];
  };

  c = evaluated.config;

  checks = {
    postgresEnabled = c.services.postgresql.enable;
    databaseEnsured = builtins.elem "agent_config" c.services.postgresql.ensureDatabases;
    userEnsured = builtins.any (u: u.name == "agent") c.services.postgresql.ensureUsers;
    # Loopback default ⇒ the firewall port stays closed.
    firewallClosedByDefault = !(builtins.elem 5432 (c.networking.firewall.allowedTCPPorts or [ ]));
    # SCRAM required (the hba lines depend on it).
    scramEncryption = (c.services.postgresql.settings.password_encryption or "") == "scram-sha-256";
    # Tuning threaded through to the server settings.
    tuningApplied = (c.services.postgresql.settings.shared_buffers or "") == "256MB";
  };

  failed = lib.filterAttrs (_: v: !v) checks;
  ok = failed == { };
in
pkgs.runCommand "nixos-agent-postgres-eval" { } (
  if ok then
    ''
      echo "nixosModules.agent-postgres evaluates: postgresql enabled, agent_config DB + agent role ensured, loopback firewall closed, scram + tuning applied" > "$out"
    ''
  else
    throw "nixos-agent-postgres-eval: unmet assertions: ${lib.concatStringsSep ", " (lib.attrNames failed)}"
)
