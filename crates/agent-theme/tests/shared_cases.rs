//! The colour and font tables shared with the portal
//! (`test/themes/validation_cases.json`): the Rust validator must agree with
//! every row, and the Dart mapper's tests load the same file.

use agent_theme::{check_font, parse_color, ThemeError};
use serde_json::Value;
use std::path::PathBuf;

fn cases(table: &str) -> Vec<Value> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/themes/validation_cases.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).expect("validation_cases.json is JSON");
    let rows = doc[table]
        .as_array()
        .unwrap_or_else(|| panic!("no table {table}"))
        .clone();
    // Every table carries all five classes.
    for class in [
        "positive_",
        "negative_",
        "boundary_",
        "corner_",
        "adversarial_",
    ] {
        assert!(
            rows.iter()
                .any(|r| r["name"].as_str().unwrap().starts_with(class)),
            "{table} has no {class} row"
        );
    }
    rows
}

fn input(row: &Value) -> String {
    match row.get("input") {
        Some(v) => v.as_str().expect("input is a string").to_owned(),
        None => {
            let count = usize::try_from(row["count"].as_u64().expect("count")).unwrap();
            format!(
                "{}{}",
                row["prefix"].as_str().unwrap(),
                row["repeat"].as_str().unwrap().repeat(count)
            )
        }
    }
}

/// `"ok:<v>"` or `"err:<Kind>"`, so both sides compare as one string.
fn expected(row: &Value) -> String {
    let e = &row["expected"];
    match (e.get("ok"), e.get("err")) {
        (Some(v), None) => format!("ok:{}", v.as_str().unwrap()),
        (None, Some(k)) => format!("err:{}", k.as_str().unwrap()),
        _ => panic!("{}: expected needs exactly one of ok/err", row["name"]),
    }
}

fn outcome<T>(r: Result<T, ThemeError>, ok: impl Fn(T) -> String) -> String {
    match r {
        Ok(v) => format!("ok:{}", ok(v)),
        Err(e) => format!("err:{}", e.kind().as_str()),
    }
}

fn run(table: &str, f: impl Fn(&str) -> String) {
    let mut failures = Vec::new();
    for row in cases(table) {
        let got = f(&input(&row));
        let want = expected(&row);
        if got != want {
            failures.push(format!(
                "{} ({}): got {got}, want {want}",
                row["name"], row["description"]
            ));
        }
    }
    assert!(failures.is_empty(), "{table}:\n{}", failures.join("\n"));
}

#[test]
fn parse_color_matches_shared_table() {
    run("parse_color", |s| {
        outcome(parse_color(s), |v| format!("0x{v:08X}"))
    });
}

#[test]
fn font_family_matches_shared_table() {
    run("font_family", |s| outcome(check_font(s), |()| s.to_owned()));
}
