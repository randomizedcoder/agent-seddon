# nix/checks/leak-bench-sync.nix
#
# Fails while a `crates/<crate>/tests/*leak*.rs` or `crates/<crate>/benches/*.rs` has
# no row in the `runs` table of nix/checks/leak.nix / bench.nix: such a file would
# never run in the gate. A row is `--test <stem>` / `--bench <stem>` in one of the
# crate's argument strings. (A row without a file fails its own `leak-`/`bench-`
# check, since cargo can't find the target.)
#
# `missingRows` is pure, and the `cases` table below checks it against fixtures, so
# a broken matcher fails here instead of passing vacuously.
{
  pkgs,
  lib,
  src,
  leakRuns,
  benchRuns,
}:

let
  # { crate = [ stem … ]; } for the `.rs` files in `crates/<crate>/<dir>` whose stem
  # matches `pattern`.
  filesOf =
    dir: pattern:
    let
      crates = lib.attrNames (
        lib.filterAttrs (_: t: t == "directory") (builtins.readDir (src + "/crates"))
      );
      stems =
        crate:
        let
          path = src + "/crates/${crate}/${dir}";
        in
        if builtins.pathExists path then
          map (lib.removeSuffix ".rs") (
            lib.filter (f: lib.hasSuffix ".rs" f && builtins.match pattern (lib.removeSuffix ".rs" f) != null) (
              lib.attrNames (builtins.readDir path)
            )
          )
        else
          [ ];
    in
    lib.filterAttrs (_: s: s != [ ]) (lib.genAttrs crates stems);

  # The `<dir>/<stem>.rs` files with no `<flag> <stem>` among their crate's rows.
  missingRows =
    {
      dir,
      flag,
      files,
      runs,
    }:
    lib.concatLists (
      lib.mapAttrsToList (
        crate: stems:
        map (s: "crates/${crate}/${dir}/${s}.rs") (
          lib.filter (
            s: !(lib.any (a: builtins.match "(.* )?${flag} ${s}( .*)?" a != null) (runs.${crate} or [ ]))
          ) stems
        )
      ) files
    );

  cases = [
    {
      name = "positive_row_present";
      description = "a file with its exact row is not reported";
      files.agent-a = [ "leak" ];
      runs.agent-a = [ "--features dhat-heap --test leak" ];
      expected = [ ];
    }
    {
      name = "positive_several_rows";
      description = "each of a crate's files is matched by its own row";
      files.agent-a = [
        "leak"
        "route_leak"
      ];
      runs.agent-a = [
        "--test route_leak --features x"
        "--test leak"
      ];
      expected = [ ];
    }
    {
      name = "negative_crate_missing";
      description = "a crate with a file but no table entry is reported";
      files.agent-b = [ "leak" ];
      runs = { };
      expected = [ "crates/agent-b/tests/leak.rs" ];
    }
    {
      name = "negative_one_of_two_missing";
      description = "only the file without a row is reported";
      files.agent-a = [
        "leak"
        "branch_leak"
      ];
      runs.agent-a = [ "--test leak" ];
      expected = [ "crates/agent-a/tests/branch_leak.rs" ];
    }
    {
      name = "corner_no_files";
      description = "no files means nothing to report, whatever the table holds";
      files = { };
      runs.agent-a = [ "--test leak" ];
      expected = [ ];
    }
    {
      name = "boundary_prefix_is_not_a_match";
      description = "`--test leak` does not count as a row for `leak_extra`";
      files.agent-a = [ "leak_extra" ];
      runs.agent-a = [ "--test leak" ];
      expected = [ "crates/agent-a/tests/leak_extra.rs" ];
    }
    {
      name = "boundary_suffix_is_not_a_match";
      description = "`--test route_leak` does not count as a row for `leak`";
      files.agent-a = [ "leak" ];
      runs.agent-a = [ "--test route_leak" ];
      expected = [ "crates/agent-a/tests/leak.rs" ];
    }
    {
      name = "adversarial_other_crates_row";
      description = "another crate's row does not cover this crate's file";
      files.agent-a = [ "leak" ];
      runs.agent-b = [ "--test leak" ];
      expected = [ "crates/agent-a/tests/leak.rs" ];
    }
    {
      name = "adversarial_stem_in_feature_list";
      description = "the stem appearing as a feature name, not after the flag, is no row";
      files.agent-a = [ "leak" ];
      runs.agent-a = [ "--features leak --test other" ];
      expected = [ "crates/agent-a/tests/leak.rs" ];
    }
  ];

  failedCases = lib.filter (
    c:
    missingRows {
      dir = "tests";
      flag = "--test";
      inherit (c) files runs;
    } != c.expected
  ) cases;

  missing =
    missingRows {
      dir = "tests";
      flag = "--test";
      files = filesOf "tests" ".*leak.*";
      runs = leakRuns;
    }
    ++ missingRows {
      dir = "benches";
      flag = "--bench";
      files = filesOf "benches" ".*";
      runs = benchRuns;
    };
in
pkgs.runCommand "leak-bench-sync-check" { } (
  if failedCases != [ ] then
    ''
      echo "leak-bench-sync: the matcher failed its own cases:" >&2
      ${lib.concatMapStrings (c: "echo '  ${c.name}: ${c.description}' >&2\n") failedCases}
      exit 1
    ''
  else if missing != [ ] then
    ''
      echo "These files have no row in nix/checks/leak.nix / bench.nix, so the gate never runs them:" >&2
      ${lib.concatMapStrings (f: "echo '  ${f}' >&2\n") missing}
      exit 1
    ''
  else
    "touch $out"
)
