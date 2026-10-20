# RK-06: execute real-Git I1 and runtime config/CLI composition with graph support enabled.
{
  pkgs,
  craneLib,
  commonArgs,
  cargoArtifacts,
}:
craneLib.mkCargoDerivation (
  commonArgs
  // {
    inherit cargoArtifacts;
    pname = "repo-graph-index";
    version = "0.1.0";
    doInstallCargoArtifacts = false;
    nativeBuildInputs = (commonArgs.nativeBuildInputs or [ ]) ++ [
      pkgs.git
      pkgs.python3
    ];
    buildPhaseCargoCommand = ''
      python3 ${../pg-suite-tests.py} ${../pg-suite.py}
      cargo test -p agent-repo-graph --features index --test indexing -- --list | tee cases.txt
      test "$(grep -c ': test$' cases.txt)" -ge 10
      cargo test -p agent-repo-graph --features index --test indexing
      cargo test -p agent-runtime --features repo-graph repo_index
      cargo test -p agent-cli --features repo-graph repo_commands
      cargo run -p agent-cli --features repo-graph -- repo --help
    '';
    installPhaseCommand = "mkdir -p $out";
  }
)
