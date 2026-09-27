use super::*;
use rstest::rstest;

const KEY: &str = "s3cr3t-value";
const ENV: &str = "AGENT_SEDDON_S17_TEST_KEY";

/// `root/acme/key` and `root/other/key` hold a key; `root/acme/link` is a symlink
/// to the other tenant's key and `root/acme/out` one to a file outside the root.
struct Tree {
    _dir: PathBuf,
    root: PathBuf,
    outside: PathBuf,
}

fn tree() -> Tree {
    let dir = agent_testkit::tempdir();
    let root = dir.join("secrets");
    for t in ["acme", "other"] {
        std::fs::create_dir_all(root.join(t)).unwrap();
        std::fs::write(root.join(t).join("key"), format!("{KEY}-{t}\n")).unwrap();
    }
    let outside = dir.join("host.key");
    std::fs::write(&outside, "host-secret").unwrap();
    std::os::unix::fs::symlink(root.join("other/key"), root.join("acme/link")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("acme/out")).unwrap();
    std::os::unix::fs::symlink(root.join("acme/nope"), root.join("acme/dangling")).unwrap();
    Tree {
        _dir: dir,
        root,
        outside,
    }
}

fn confined(t: &Tree, allow_env: bool) -> SecretsPolicy {
    SecretsPolicy {
        per_tenant: true,
        root: t.root.clone(),
        allow_env_for_tenants: allow_env,
    }
}

#[rstest]
#[case::positive_relative_file("file:key", Some("s3cr3t-value-acme"))]
#[case::positive_absolute_file_inside(":abs:acme/key", Some("s3cr3t-value-acme"))]
#[case::positive_empty_ref_is_absent("", Some(""))]
#[case::negative_missing_file_inside("file:nope", None)]
#[case::adversarial_absolute_outside_root(":outside", None)]
#[case::adversarial_etc_passwd("file:/etc/passwd", None)]
#[case::adversarial_dotdot_escape("file:../other/key", None)]
#[case::adversarial_dotdot_inside_absolute(":abs:acme/../other/key", None)]
#[case::adversarial_other_tenants_dir(":abs:other/key", None)]
#[case::adversarial_symlink_to_other_tenant("file:link", None)]
#[case::adversarial_symlink_escape("file:out", None)]
#[case::adversarial_dangling_symlink("file:dangling", None)]
#[case::adversarial_the_dir_itself(":abs:acme", None)]
#[case::adversarial_raw_secret_not_a_ref("ghp_rawtokenvalue", None)]
fn tenant_file_refs(#[case] raw: &str, #[case] want: Option<&str>) {
    let t = tree();
    // `:abs:<rel>` ⇒ an absolute path under the root; `:outside` ⇒ the host file.
    let raw = if let Some(rel) = raw.strip_prefix(":abs:") {
        format!("file:{}", t.root.join(rel).display())
    } else if raw == ":outside" {
        format!("file:{}", t.outside.display())
    } else {
        raw.to_string()
    };
    let got = resolve_with(&confined(&t, false), SecretScope::Tenant("acme"), &raw);
    match want {
        Some(v) => assert_eq!(got.expect("resolves").expose(), v),
        None => {
            let err = got.expect_err("refused");
            // Never echo where it pointed.
            assert!(!err.contains(&t.root.display().to_string()), "{err}");
            assert!(
                !err.contains("passwd") && !err.contains("host.key"),
                "{err}"
            );
        }
    }
}

#[rstest]
#[case::adversarial_env_refused_by_default(false, false)]
#[case::positive_env_when_allowed(true, true)]
fn tenant_env_refs(#[case] allow_env: bool, #[case] resolves: bool) {
    let t = tree();
    std::env::set_var(ENV, KEY);
    let got = resolve_with(
        &confined(&t, allow_env),
        SecretScope::Tenant("acme"),
        &format!("env:{ENV}"),
    );
    match got {
        Ok(s) => {
            assert!(resolves);
            assert_eq!(s.expose(), KEY);
        }
        Err(e) => {
            assert!(!resolves);
            assert!(!e.contains(ENV), "the variable name is not echoed: {e}");
        }
    }
}

#[rstest]
#[case::adversarial_traversal_tenant("..")]
#[case::adversarial_slash_tenant("acme/../other")]
#[case::boundary_empty_tenant("")]
fn adversarial_bad_tenant_segment_rejected(#[case] tenant: &str) {
    let t = tree();
    assert!(resolve_with(&confined(&t, true), SecretScope::Tenant(tenant), "file:key").is_err());
}

#[rstest]
#[case::positive_operator_env_ref(SecretScope::Operator, true)]
#[case::corner_tenant_with_per_tenant_off(SecretScope::Tenant("local"), false)]
fn unconfined_scopes_keep_legacy_resolution(
    #[case] scope: SecretScope<'_>,
    #[case] per_tenant: bool,
) {
    let t = tree();
    let policy = SecretsPolicy {
        per_tenant,
        ..confined(&t, false)
    };
    std::env::set_var(ENV, KEY);
    assert_eq!(
        resolve_with(&policy, scope, &format!("env:{ENV}"))
            .unwrap()
            .expose(),
        KEY
    );
    let host = format!("file:{}", t.outside.display());
    assert_eq!(
        resolve_with(&policy, scope, &host).unwrap().expose(),
        "host-secret"
    );
}

#[test]
fn corner_unset_env_is_absent_not_an_error() {
    let got = resolve_with(
        &SecretsPolicy::default(),
        SecretScope::Operator,
        "env:AGENT_SEDDON_S17_DEFINITELY_UNSET",
    );
    assert_eq!(got.unwrap().expose(), "");
}

#[test]
fn negative_operator_missing_file_is_an_error() {
    let got = resolve_with(
        &SecretsPolicy::default(),
        SecretScope::Operator,
        "file:/no/such/agent-seddon/secret",
    );
    assert!(got.unwrap_err().contains("/no/such/agent-seddon/secret"));
}

#[test]
fn boundary_admit_returns_the_confined_path() {
    let t = tree();
    let got = admit(
        &confined(&t, false),
        SecretScope::Tenant("acme"),
        "file:key",
    )
    .unwrap();
    assert_eq!(got, Admitted::File(t.root.join("acme").join("key")));
}
