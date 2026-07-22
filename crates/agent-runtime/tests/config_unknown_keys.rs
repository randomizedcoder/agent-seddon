//! Unknown config keys are reported rather than silently ignored.
//!
//! The config selects which implementation each seam uses, so a misplaced key
//! means the agent quietly runs something other than what the operator asked
//! for. This is the guard for that, plus the guard on the guard: the shipped
//! reference config must produce **no** warnings, or the feature is just noise
//! everyone learns to ignore.

use agent_runtime::parse_config_reporting_unknown;
use rstest::rstest;

/// The reference config must be clean. If this fails, either a key was renamed
/// without updating `config/agent.toml`, or the config grew a key that is not
/// actually read — both worth knowing.
#[test]
fn positive_the_shipped_reference_config_has_no_unknown_keys() {
    let toml = include_str!("../../../config/agent.toml");
    let (_cfg, unknown) = parse_config_reporting_unknown(toml).expect("the shipped config parses");
    assert!(
        unknown.is_empty(),
        "config/agent.toml contains keys nothing reads: {unknown:?}"
    );
}

/// The exact mistake that motivated this: a real key in the wrong section. It
/// parsed cleanly before, and the agent used the local memory store.
#[test]
fn negative_a_key_in_the_wrong_section_is_reported() {
    let toml = r#"
        [agent]
        provider = "anthropic"
        memory   = "grpc"

        [provider]
        model = "m"
    "#;
    let (_cfg, unknown) = parse_config_reporting_unknown(toml).expect("parses");
    assert_eq!(
        unknown,
        vec!["agent.memory"],
        "a misplaced key must be reported with its full path"
    );
}

/// Misspellings, stale keys, and unknown sections are all reported — with the
/// dotted path, so the message says where to look.
#[rstest]
#[case::negative_misspelled_field(
    r#"
    [agent]
    provider = "anthropic"
    [provider]
    model = "m"
    [tokenizer]
    backendd = "approx"
    "#,
    "tokenizer.backendd"
)]
#[case::negative_unknown_section(
    r#"
    [agent]
    provider = "anthropic"
    [provider]
    model = "m"
    [nonesuch]
    x = 1
    "#,
    "nonesuch"
)]
#[case::negative_stale_nested_key(
    r#"
    [agent]
    provider = "anthropic"
    [provider]
    model = "m"
    [grpc.policy]
    endpointt = "x"
    "#,
    "grpc.policy.endpointt"
)]
fn negative_unknown_keys_are_reported_with_their_path(#[case] toml: &str, #[case] expected: &str) {
    let (_cfg, unknown) = parse_config_reporting_unknown(toml).expect("parses");
    assert!(
        unknown.iter().any(|k| k == expected),
        "expected `{expected}` to be reported, got {unknown:?}"
    );
}

/// A config that uses only real keys must stay quiet — including optional
/// sections left at their defaults.
#[test]
fn positive_a_correct_config_reports_nothing() {
    let toml = r#"
        [agent]
        provider = "anthropic"
        policy   = "auto-approve"
        [provider]
        model = "m"
        [tokenizer]
        backend = "approx"
        [grpc.policy]
        endpoint = "127.0.0.1:50055"
    "#;
    let (_cfg, unknown) = parse_config_reporting_unknown(toml).expect("parses");
    assert!(unknown.is_empty(), "false positives: {unknown:?}");
}

/// Unknown keys must remain a WARNING, not an error — rejecting them would turn
/// a stale key into a hard startup failure and break configs that work today.
#[test]
fn positive_an_unknown_key_still_parses_successfully() {
    let toml = r#"
        [agent]
        provider = "anthropic"
        [provider]
        model = "m"
        [totally]
        unknown = true
    "#;
    assert!(
        agent_runtime::parse_config(toml).is_ok(),
        "an unknown key must not fail the parse"
    );
}
