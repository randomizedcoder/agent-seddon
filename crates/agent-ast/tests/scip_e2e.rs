//! End-to-end SCIP ingestion over **real `scip-go` output**: index a tiny Go module
//! with an interface and two implicit implementers, then assert the `ScipAst` model
//! resolves the implementation relation. Self-skips when `scip-go`/`go` aren't on
//! PATH (so a plain `cargo test --features ast-scip` passes locally); the hermetic
//! `nix/checks/ast-scip.nix` runs it with both pinned on PATH.

#![cfg(feature = "ast-scip")]

use agent_ast::model::SymbolModel;
use agent_core::SymbolRef;
use std::process::Command;

fn on_path(bin: &str) -> bool {
    Command::new(bin)
        .arg("--help")
        .output()
        .map(|_| true)
        .unwrap_or(false)
}

#[test]
fn scip_go_implementations_end_to_end() {
    if !on_path("scip-go") || !on_path("go") {
        eprintln!("skipping: scip-go/go not on PATH");
        return;
    }

    let dir = agent_testkit::tempdir();
    std::fs::create_dir_all(dir.join("greeter")).unwrap();
    std::fs::write(dir.join("go.mod"), "module example.com/fx\n\ngo 1.24\n").unwrap();
    std::fs::write(
        dir.join("greeter/greeter.go"),
        r#"package greeter

type Greeter interface {
	Greet(name string) string
}

type Polite struct{ Prefix string }

func (p Polite) Greet(name string) string { return p.Prefix + " " + name }

type Loud struct{}

func (l *Loud) Greet(name string) string { return name + "!!!" }
"#,
    )
    .unwrap();

    // Hermetic Go env under the temp dir; stdlib-only ⇒ no network.
    let run = |bin: &str, args: &[&str]| {
        Command::new(bin)
            .args(args)
            .current_dir(&dir)
            .env("HOME", &dir)
            .env("GOPATH", dir.join("gopath"))
            .env("GOCACHE", dir.join("gocache"))
            .env("GOFLAGS", "-mod=mod")
            .env("GOPROXY", "off")
            .env("GO111MODULE", "on")
            .output()
            .unwrap()
    };
    run("go", &["mod", "tidy"]);
    let out = run("scip-go", &["--output", "index.scip"]);
    assert!(
        dir.join("index.scip").exists(),
        "scip-go produced no index: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let bytes = std::fs::read(dir.join("index.scip")).unwrap();
    let mut m = SymbolModel::default();
    assert!(m.ingest_scip_bytes(&bytes, &dir, "go"), "decode scip index");

    let impls = m.implementations(&SymbolRef::name("Greeter"));
    let names: Vec<&str> = impls.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"Polite"),
        "Polite implements Greeter; got {names:?}"
    );
    assert!(
        names.contains(&"Loud"),
        "Loud implements Greeter; got {names:?}"
    );
}
