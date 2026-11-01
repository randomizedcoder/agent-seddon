# nix/workspace-slice.nix
#
# Per-crate source slices, so a check rebuilds only when a crate it can see changes.
#
# Every crane check used to take the whole-workspace source, so touching any `.rs`
# file moved every check's hash and the gate re-ran all of them. A slice keeps the
# real sources of one crate's workspace-dependency closure (normal, dev and build
# deps, so the tests compile) and replaces every other member with crane's dummy
# stub (`mkDummySrc`: manifest + empty target), so cargo still sees the whole
# workspace and the shared `Cargo.lock`.
#
# Each piece is a filtered *path*, so its store path follows its content only: a
# slice's hash moves when one of its crates, the root manifest/lockfile, or the
# shared non-crate inputs (config textprotos, portal assets, …) change, and not
# otherwise.
#
# The crates nearly everything depends on (`foundation`) are compiled once into
# `foundationArtifacts`, which every slice reuses, so a cold gate doesn't compile
# `agent-core` once per crate. Cargo identifies a workspace crate by its absolute
# path, so every slice unpacks to the same `/build/source` (they are all named
# `source`) and the foundation's compiled crates stay fresh in each of them.
#
{
  lib,
  pkgs,
  craneLib,
  # The repo root (a path), used to read manifests at eval time.
  root,
  # The filtered whole-workspace source (nix/default.nix `cleanedSrc`).
  cleanedSrc,
  # The shared crane arguments and the external-dependency build.
  commonArgs,
  cargoArtifacts,
}:

let
  readToml = p: builtins.fromTOML (builtins.readFile p);

  workspace = readToml (root + "/Cargo.toml");
  memberDirs = workspace.workspace.members;

  manifestOf = dir: readToml (root + "/${dir}/Cargo.toml");
  nameOf = dir: (manifestOf dir).package.name;

  # crate name -> member dir ("crates/agent-core").
  dirByName = lib.listToAttrs (
    map (dir: {
      name = nameOf dir;
      value = dir;
    }) memberDirs
  );

  # Workspace crates a manifest depends on, across every dependency table
  # (including `[target.'cfg(..)'.*]`). A renamed dependency resolves through
  # `package = "…"`.
  depTables = m: [
    (m.dependencies or { })
    (m.dev-dependencies or { })
    (m.build-dependencies or { })
  ];
  allDepTables = m: depTables m ++ lib.concatMap depTables (lib.attrValues (m.target or { }));
  workspaceDepsOf =
    name:
    let
      m = manifestOf dirByName.${name};
      deps = lib.concatMap (
        t:
        lib.mapAttrsToList (
          key: spec: if builtins.isAttrs spec && spec ? package then spec.package else key
        ) t
      ) (allDepTables m);
    in
    lib.filter (d: dirByName ? ${d}) (lib.unique deps);

  # The crate plus every workspace crate it (transitively) depends on.
  closureOf =
    names:
    map (x: x.key) (
      builtins.genericClosure {
        startSet = map (n: { key = n; }) names;
        operator = x: map (n: { key = n; }) (workspaceDepsOf x.key);
      }
    );

  # Crates with the most dependents (agent-core alone has 47). Edit one and every
  # slice re-runs, which is right: each of them compiles against it.
  foundation = [
    "agent-core"
    "agent-metrics"
    "agent-mcp"
    "agent-testkit"
    "agent-config-store"
    "agent-proto"
    "agent-retry"
  ];

  # Crates whose *files* a crate's tests read without depending on them, so the
  # slice must carry their real source (never compiled, only read). A missing entry
  # fails loudly: `include_str!` won't compile, and the source-scanning guards
  # reject a dir of crane stubs.
  extraSources = {
    # postgres tests apply the config-store migration (`include_str!`).
    agent-campaign = [ "agent-config-store" ];
    # `no_unscoped_spawn` scans the served paths.
    agent-grpc = [
      "agent-runtime"
      "agent-review-fleet"
    ];
    # `no_raw_spawn` scans the Sandbox-chokepointed crates.
    agent-tools = [
      "agent-git"
      "agent-search"
    ];
  };

  # The foundation crates' features in a whole-workspace build, so the slices that
  # turn them on reuse these artifacts instead of recompiling.
  testFeatures = lib.importJSON (root + "/nix/test-features.json");
  foundationFeatures = lib.concatMap (c: map (f: "${c}/${f}") (testFeatures.${c} or [ ])) foundation;

  # The cleaned source restricted to paths under one prefix (or outside `crates/`).
  under =
    prefix:
    lib.cleanSourceWith {
      src = cleanedSrc;
      filter =
        path: _type:
        let
          rel = lib.removePrefix (toString cleanedSrc.origSrc or cleanedSrc) (toString path);
        in
        lib.hasPrefix "/${prefix}/" "${rel}/" || lib.hasPrefix "${rel}/" "/${prefix}/";
      name = "source-${lib.replaceStrings [ "/" ] [ "-" ] prefix}";
    };

  # Everything outside `crates/` (root manifest + lockfile, config textprotos,
  # portal assets, deny.toml, …).
  shared = lib.cleanSourceWith {
    src = cleanedSrc;
    filter =
      path: _type:
      let
        rel = lib.removePrefix (toString cleanedSrc.origSrc or cleanedSrc) (toString path);
      in
      !(lib.hasPrefix "/crates/" rel);
    name = "source-shared";
  };

  dummy = craneLib.mkDummySrc { src = cleanedSrc; };

  # A source tree with the real code for `names`' closure and stubs elsewhere.
  sliceFor =
    names:
    let
      dirs = map (n: dirByName.${n}) (
        lib.unique (closureOf names ++ lib.concatMap (n: extraSources.${n} or [ ]) names)
      );
    in
    pkgs.runCommand "source" { } ''
      cp -r ${dummy} $out
      chmod -R u+w $out
      cp -rT ${shared} $out
      ${lib.concatMapStrings (d: ''
        rm -rf $out/${d}
        cp -r ${under d}/${d} $out/${d}
      '') dirs}
    '';

  # The foundation crates, compiled (with their tests) on top of the external deps.
  foundationArtifacts = craneLib.cargoBuild (
    commonArgs
    // {
      pname = "workspace-foundation";
      src = sliceFor foundation;
      inherit cargoArtifacts;
      cargoExtraArgs =
        lib.concatMapStringsSep " " (c: "-p ${c}") foundation
        + " --all-targets"
        + lib.optionalString (
          foundationFeatures != [ ]
        ) " --features ${lib.concatStringsSep "," foundationFeatures}";
      doCheck = false;
    }
  );

  # The foundation crates linted, for the `clippy-<crate>` checks to start from:
  # clippy checks workspace crates through `clippy-driver`, so a plain build's
  # artifacts don't count as linted. Same features as `foundationArtifacts`.
  foundationClippyArtifacts = craneLib.cargoClippy (
    commonArgs
    // {
      pname = "workspace-foundation";
      src = sliceFor foundation;
      inherit cargoArtifacts;
      cargoClippyExtraArgs =
        lib.concatMapStringsSep " " (c: "-p ${c}") foundation
        + " --all-targets"
        + lib.optionalString (
          foundationFeatures != [ ]
        ) " --features ${lib.concatStringsSep "," foundationFeatures}"
        + " -- -D warnings";
    }
  );

  # Arguments that point a crane check at one slice, on top of the foundation.
  sliceArgs = names: {
    src = sliceFor names;
    cargoArtifacts = foundationArtifacts;
  };

  # The same, for a clippy check (on top of the linted foundation).
  clippySliceArgs = names: {
    src = sliceFor names;
    cargoArtifacts = foundationClippyArtifacts;
  };
in
{
  inherit
    closureOf
    sliceFor
    sliceArgs
    clippySliceArgs
    dummy
    extraSources
    foundation
    foundationArtifacts
    foundationClippyArtifacts
    ;
  crateNames = lib.attrNames dirByName;
}
