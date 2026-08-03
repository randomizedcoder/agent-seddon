# nix/lib/default.nix
#
# Shared helpers for the flake — the one place cross-cutting nix/shell patterns
# live, so the app/check modules state only what varies. Imported once from
# nix/default.nix as `nixLib = import ./lib { inherit pkgs lib versions; }`.
{
  pkgs,
  lib,
  versions,
}:

{
  # A flake `apps.<name>` record. `drv` is the derivation, `bin` its executable
  # under `$out/bin` (usually equal to the app name).
  mkApp = drv: bin: {
    type = "app";
    program = "${drv}/bin/${bin}";
  };

  # Turn an attrset of `{ appName = derivation; }` into `{ appName = app; }`.
  # `bins` overrides the executable name for apps whose binary differs from the
  # app name (e.g. `clickhouse-client` → `clickhouse-client-wrapper`); every other
  # app's binary is assumed to match its name.
  mkApps =
    bins: drvs:
    builtins.mapAttrs (name: drv: {
      type = "app";
      program = "${drv}/bin/${bins.${name} or name}";
    }) drvs;

  # Sourced bash snippets for the opt-in shell harnesses, concatenated into a
  # `writeShellApplication`'s `text` (after `set -uo pipefail`) so shellcheck sees
  # the whole script — no runtime `source`, no SC1091. See each file's header.
  harness = {
    contract = builtins.readFile ./contract.sh; # worst / note_fail / contract_exit
    serveWire = builtins.readFile ./serve-wire.sh; # work + serve-all boot/health/dial
  };
}
