//! agent-k8s-secrets — build Kubernetes `Secret` manifests from local files, safely.
//!
//! Design: docs/design/k8s/07-secrets.md.
//!
//! Secret material must never enter **git** or the **Nix store** — both are readable
//! forever. So, unlike the rest of the k8s track, secrets are *not* rendered into the
//! committed `rendered/` tree. Instead `nix run .#k8s-secrets` reads source files on
//! the deploying host at run time and pipes the built `Secret`s straight into
//! `kubectl apply --server-side -f -`: no temp file, no store path.
//!
//! This crate is the pure, testable core — manifest parsing, **fail-closed** source
//! validation, `Secret` construction, and the redaction guarantee that no secret byte
//! ever reaches an error string or a dry-run summary. The binary (`src/main.rs`) is the
//! thin imperative shell that loads the manifest, calls this core, and drives `kubectl`.
//!
//! ## Threat model
//!
//! The manifest is operator-authored (mode `0600`, outside the repo), not
//! model-controlled — so this is not the LLM-untrusted surface `confine`/`safe_segment`
//! guard (CLAUDE.md). But a deploy tool that reads key material still fails closed, per
//! 07-secrets.md: a source file that is group/world-readable, oversized, a symlink that
//! escapes the allowed roots, or keyed by an unsafe name, is **refused**, and no secret
//! value is ever echoed. Each rule is proven live by an `adversarial_` case below.

use base64::Engine as _;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Max source-file size. Secrets are keys/tokens/DSNs — kilobytes, not megabytes; a
/// larger file is almost certainly a mistake (a wrong path, a whole keyring), so it is
/// refused rather than base64'd into etcd.
pub const DEFAULT_SIZE_CAP: u64 = 1 << 20; // 1 MiB

/// The one fixed label every managed Secret carries (the ArgoCD app-of-apps tracks
/// `part-of`, but these objects get **no** tracking annotation — see [`SecretManifest`]).
pub const PART_OF: &str = "agent-seddon";
/// Marks the Secret as ours so an operator can list what this tool manages.
pub const MANAGED_BY_LABEL: &str = "agent-seddon.io/managed-by";
/// Value of [`MANAGED_BY_LABEL`].
pub const MANAGED_BY: &str = "k8s-secrets";

/// Everything that can go wrong building a Secret. **No variant carries secret bytes**:
/// the fields are names, paths, sizes and modes — never file contents — so the `Display`
/// text is safe to print to stderr. (Proven by `adversarial_no_secret_value_in_output`.)
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("manifest is not valid TOML: {0}")]
    ManifestParse(String),
    #[error("secret {secret:?} key {key:?}: source file {path:?} does not exist")]
    MissingFile {
        secret: String,
        key: String,
        path: PathBuf,
    },
    #[error("secret {secret:?} key {key:?}: source {path:?} resolves outside the allowed roots")]
    OutsideRoots {
        secret: String,
        key: String,
        path: PathBuf,
    },
    #[error(
        "secret {secret:?} key {key:?}: source {path:?} is group/other-readable (mode {mode:#o}); chmod 600 it"
    )]
    TooPermissive {
        secret: String,
        key: String,
        path: PathBuf,
        mode: u32,
    },
    #[error(
        "secret {secret:?} key {key:?}: source {path:?} is {size} bytes, over the {cap}-byte cap"
    )]
    TooLarge {
        secret: String,
        key: String,
        path: PathBuf,
        size: u64,
        cap: u64,
    },
    #[error("secret {secret:?}: {key:?} is not a valid Secret data key (expected [-._a-zA-Z0-9]+, not '.'/'..', no '/')")]
    UnsafeKey { secret: String, key: String },
    #[error("secret {secret:?} key {key:?}: reading {path:?} failed: {kind}")]
    Io {
        secret: String,
        key: String,
        path: PathBuf,
        // std::io::Error's Display names the condition, never file contents.
        kind: String,
    },
}

/// One `key -> file` mapping inside a Secret. `optional` keys that are absent on the
/// host are skipped (07-secrets.md's `previous.pem`); a present-but-invalid optional
/// file is still refused (optionality excuses absence, never a bad file).
#[derive(Debug, Clone)]
pub struct KeySpec {
    pub key: String,
    pub path: PathBuf,
    pub optional: bool,
}

/// One `Secret` to build: a name and its `key -> file` mappings.
#[derive(Debug, Clone)]
pub struct SecretSpec {
    pub name: String,
    pub keys: Vec<KeySpec>,
}

/// A parsed `k8s-secrets.toml`: an optional namespace, the allowed source roots a file
/// must resolve within, and the ordered Secrets.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub namespace: Option<String>,
    pub allowed_roots: Vec<PathBuf>,
    pub secrets: Vec<SecretSpec>,
}

/// A built `Secret` manifest. Note what is **absent**: any `metadata.annotations`. ArgoCD
/// prunes objects carrying its tracking annotation; these Secrets deliberately carry none,
/// so a GitOps sync never deletes the operator's out-of-band key material.
#[derive(Debug, Serialize)]
pub struct SecretManifest {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: SecretMeta,
    #[serde(rename = "type")]
    pub secret_type: String,
    /// `key -> base64(bytes)`. This IS secret-derived; it is only ever serialized into
    /// the `kubectl` stdin pipe, never logged.
    pub data: BTreeMap<String, String>,
}

/// `Secret.metadata` — name, namespace, labels; no annotations by construction.
#[derive(Debug, Serialize)]
pub struct SecretMeta {
    pub name: String,
    pub namespace: String,
    pub labels: BTreeMap<String, String>,
}

impl SecretManifest {
    /// The YAML document piped to `kubectl apply`. Contains the base64 data by design.
    #[must_use]
    pub fn to_yaml(&self) -> String {
        // Plain structs + string maps: serialization is infallible here.
        serde_yaml_ng::to_string(self).expect("serialize Secret manifest")
    }
}

/// Parse a `k8s-secrets.toml`. **Fail closed:** any TOML error, or a secret table whose
/// entry is neither a string path nor a `{ file, optional }` table, is rejected — a
/// malformed manifest never silently yields a partial or empty Secret set.
///
/// Shape:
/// ```toml
/// namespace = "agent-seddon"            # optional
/// allowed_roots = ["/home/op/.config"]  # optional; defaults supplied by the caller
///
/// [agent-signing-key]
/// "signing.pem"  = "/home/op/.config/agent-seddon/signing.pem"
/// "previous.pem" = { file = "/home/op/.config/agent-seddon/previous.pem", optional = true }
/// ```
///
/// # Errors
/// [`Error::ManifestParse`] on invalid TOML or an unrecognized entry shape.
pub fn parse_manifest(text: &str) -> Result<Manifest, Error> {
    let table: toml::Table = text
        .parse()
        .map_err(|e| Error::ManifestParse(format!("{e}")))?;

    let mut namespace = None;
    let mut allowed_roots = Vec::new();
    let mut secrets = Vec::new();

    for (name, value) in table {
        match name.as_str() {
            "namespace" => {
                let ns = value.as_str().ok_or_else(|| {
                    Error::ManifestParse("`namespace` must be a string".to_string())
                })?;
                namespace = Some(ns.to_string());
            }
            "allowed_roots" => {
                let arr = value.as_array().ok_or_else(|| {
                    Error::ManifestParse("`allowed_roots` must be an array of strings".to_string())
                })?;
                for item in arr {
                    let s = item.as_str().ok_or_else(|| {
                        Error::ManifestParse("`allowed_roots` entries must be strings".to_string())
                    })?;
                    allowed_roots.push(PathBuf::from(s));
                }
            }
            _ => secrets.push(parse_secret(&name, &value)?),
        }
    }

    // A stable order regardless of the map's iteration order keeps output deterministic.
    secrets.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Manifest {
        namespace,
        allowed_roots,
        secrets,
    })
}

fn parse_secret(name: &str, value: &toml::Value) -> Result<SecretSpec, Error> {
    let table = value.as_table().ok_or_else(|| {
        Error::ManifestParse(format!("`{name}` must be a table of key = file mappings"))
    })?;
    let mut keys = Vec::new();
    for (key, entry) in table {
        let spec = match entry {
            toml::Value::String(path) => KeySpec {
                key: key.clone(),
                path: PathBuf::from(path),
                optional: false,
            },
            toml::Value::Table(t) => {
                let path = t.get("file").and_then(toml::Value::as_str).ok_or_else(|| {
                    Error::ManifestParse(format!("`{name}.{key}` table needs a string `file`"))
                })?;
                let optional = t
                    .get("optional")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false);
                KeySpec {
                    key: key.clone(),
                    path: PathBuf::from(path),
                    optional,
                }
            }
            _ => {
                return Err(Error::ManifestParse(format!(
                    "`{name}.{key}` must be a file path string or a {{ file, optional }} table"
                )));
            }
        };
        keys.push(spec);
    }
    keys.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(SecretSpec {
        name: name.to_string(),
        keys,
    })
}

/// A valid Kubernetes `Secret` data key: `[-._a-zA-Z0-9]+`, and never `.`/`..` (which k8s
/// reserves) or anything with a path separator. Rejecting `/` and `..` blocks a key name
/// that tries to become a path segment on mount.
fn validate_key(secret: &str, key: &str) -> Result<(), Error> {
    let ok = !key.is_empty()
        && key != "."
        && key != ".."
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(Error::UnsafeKey {
            secret: secret.to_string(),
            key: key.to_string(),
        })
    }
}

/// Canonicalize the allowed roots once (resolving symlinks), dropping any that do not
/// exist. A source file is accepted only if its own canonical path lives under one of
/// these — the containment check that defeats a symlink escaping to `/etc/shadow`.
fn canonical_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots.iter().filter_map(|r| r.canonicalize().ok()).collect()
}

/// Validate one source file and return its bytes. **Fail closed** in this order: must
/// exist; must canonicalize within an allowed root; must not be group/other-readable;
/// must be within the size cap. Only then is it read.
///
/// # Errors
/// The matching [`Error`] variant for the first rule it violates.
fn read_source(secret: &str, key: &KeySpec, roots: &[PathBuf], cap: u64) -> Result<Vec<u8>, Error> {
    let path = &key.path;

    // Resolve symlinks. A missing file (or a dangling symlink) canonicalizes to an error.
    let canon = path.canonicalize().map_err(|_| Error::MissingFile {
        secret: secret.to_string(),
        key: key.key.clone(),
        path: path.clone(),
    })?;

    // Containment: the resolved target must live under an allowed root.
    if !roots.iter().any(|root| canon.starts_with(root)) {
        return Err(Error::OutsideRoots {
            secret: secret.to_string(),
            key: key.key.clone(),
            path: path.clone(),
        });
    }

    let meta = std::fs::metadata(&canon).map_err(|e| Error::Io {
        secret: secret.to_string(),
        key: key.key.clone(),
        path: path.clone(),
        kind: e.kind().to_string(),
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = meta.permissions().mode();
        // Any group or other permission bit set ⇒ the key material is readable beyond
        // the owner. Refuse it.
        if mode & 0o077 != 0 {
            return Err(Error::TooPermissive {
                secret: secret.to_string(),
                key: key.key.clone(),
                path: path.clone(),
                mode: mode & 0o7777,
            });
        }
    }

    let size = meta.len();
    if size > cap {
        return Err(Error::TooLarge {
            secret: secret.to_string(),
            key: key.key.clone(),
            path: path.clone(),
            size,
            cap,
        });
    }

    std::fs::read(&canon).map_err(|e| Error::Io {
        secret: secret.to_string(),
        key: key.key.clone(),
        path: path.clone(),
        kind: e.kind().to_string(),
    })
}

/// Build one `Secret` manifest from its spec. Required keys must resolve; an `optional`
/// key whose file is absent is skipped; every present file is validated by [`read_source`].
///
/// # Errors
/// The first [`Error`] from key validation or source reading.
pub fn build_secret(
    spec: &SecretSpec,
    namespace: &str,
    roots: &[PathBuf],
    cap: u64,
) -> Result<SecretManifest, Error> {
    let canon = canonical_roots(roots);
    let mut data = BTreeMap::new();

    for key in &spec.keys {
        validate_key(&spec.name, &key.key)?;

        // An optional key the operator didn't provide (nothing at the path) is skipped;
        // anything present is validated normally.
        if key.optional && std::fs::symlink_metadata(&key.path).is_err() {
            continue;
        }

        let bytes = read_source(&spec.name, key, &canon, cap)?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        data.insert(key.key.clone(), encoded);
    }

    let mut labels = BTreeMap::new();
    labels.insert("app.kubernetes.io/part-of".to_string(), PART_OF.to_string());
    labels.insert(MANAGED_BY_LABEL.to_string(), MANAGED_BY.to_string());

    Ok(SecretManifest {
        api_version: "v1".to_string(),
        kind: "Secret".to_string(),
        metadata: SecretMeta {
            name: spec.name.clone(),
            namespace: namespace.to_string(),
            labels,
        },
        secret_type: "Opaque".to_string(),
        data,
    })
}

/// Build every Secret in the manifest, in order.
///
/// # Errors
/// The first [`Error`] any secret produces (fail closed — a bad file aborts the batch
/// before anything is applied).
pub fn build_all(
    manifest: &Manifest,
    namespace: &str,
    cap: u64,
) -> Result<Vec<SecretManifest>, Error> {
    manifest
        .secrets
        .iter()
        .map(|s| build_secret(s, namespace, &manifest.allowed_roots, cap))
        .collect()
}

/// A human-readable preview for `--dry-run`: the Secret names, their namespace, and the
/// data **keys** — deliberately **never the values**. Safe to print. (Proven by
/// `adversarial_no_secret_value_in_output`.)
#[must_use]
pub fn summary(manifest: &Manifest, namespace: &str) -> String {
    let mut out = String::new();
    for spec in &manifest.secrets {
        let keys: Vec<&str> = spec.keys.iter().map(|k| k.key.as_str()).collect();
        out.push_str(&format!(
            "secret {:?} (namespace {namespace:?}): {}\n",
            spec.name,
            keys.join(", ")
        ));
    }
    out
}

/// The one application namespace every deploy target uses today (k3s on l2 and
/// full-k8s alike). A `namespace` in the manifest, or `--namespace`, overrides it.
pub const DEFAULT_NAMESPACE: &str = "agent-seddon";

/// Serialize a batch of Secrets into one multi-document YAML stream for `kubectl apply`.
#[must_use]
pub fn render_stream(secrets: &[SecretManifest]) -> String {
    secrets
        .iter()
        .map(SecretManifest::to_yaml)
        .collect::<Vec<_>>()
        .join("---\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;
    use rstest::rstest;
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;

    // A known marker we assert never leaks into any human-facing string.
    const SECRET_MARKER: &str = "SUPERSECRETvalue-do-not-leak-42";

    /// Write `content` to `dir/name` with octal `mode` and return its path.
    fn write_mode(dir: &Path, name: &str, content: &[u8], mode: u32) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, content).expect("write fixture");
        fs::set_permissions(&p, fs::Permissions::from_mode(mode)).expect("chmod fixture");
        p
    }

    fn key(path: &Path, optional: bool) -> KeySpec {
        KeySpec {
            key: "token".to_string(),
            path: path.to_path_buf(),
            optional,
        }
    }

    fn one_secret(name: &str, keys: Vec<KeySpec>) -> SecretSpec {
        SecretSpec {
            name: name.to_string(),
            keys,
        }
    }

    // ---- positive --------------------------------------------------------

    #[test]
    fn positive_parses_both_entry_shapes() {
        let text = r#"
            namespace = "agent-seddon"
            allowed_roots = ["/tmp"]

            [agent-signing-key]
            "signing.pem" = "/a/signing.pem"
            "previous.pem" = { file = "/a/previous.pem", optional = true }

            [agent-forge]
            "token" = "/b/token"
        "#;
        let m = parse_manifest(text).expect("parse");
        assert_eq!(m.namespace.as_deref(), Some("agent-seddon"));
        assert_eq!(m.allowed_roots, vec![PathBuf::from("/tmp")]);
        // sorted: agent-forge before agent-signing-key
        assert_eq!(m.secrets.len(), 2);
        assert_eq!(m.secrets[0].name, "agent-forge");
        let signing = &m.secrets[1];
        assert_eq!(signing.name, "agent-signing-key");
        // keys sorted: previous.pem before signing.pem
        assert_eq!(signing.keys[0].key, "previous.pem");
        assert!(signing.keys[0].optional);
        assert!(!signing.keys[1].optional);
    }

    #[test]
    fn positive_builds_secret_with_expected_shape() {
        let dir = tempdir();
        let path = write_mode(&dir, "token", SECRET_MARKER.as_bytes(), 0o600);
        let spec = one_secret("agent-forge", vec![key(&path, false)]);
        let secret = build_secret(
            &spec,
            "agent-seddon",
            std::slice::from_ref(&dir),
            DEFAULT_SIZE_CAP,
        )
        .expect("build");

        assert_eq!(secret.api_version, "v1");
        assert_eq!(secret.kind, "Secret");
        assert_eq!(secret.secret_type, "Opaque");
        assert_eq!(secret.metadata.name, "agent-forge");
        assert_eq!(secret.metadata.namespace, "agent-seddon");
        assert_eq!(
            secret
                .metadata
                .labels
                .get("app.kubernetes.io/part-of")
                .map(String::as_str),
            Some(PART_OF)
        );
        assert_eq!(
            secret
                .metadata
                .labels
                .get(MANAGED_BY_LABEL)
                .map(String::as_str),
            Some(MANAGED_BY)
        );

        // The data is the base64 of the real bytes — and the YAML (bound for kubectl)
        // carries it, while never carrying the plaintext.
        let expected = base64::engine::general_purpose::STANDARD.encode(SECRET_MARKER.as_bytes());
        assert_eq!(secret.data.get("token"), Some(&expected));
        let yaml = secret.to_yaml();
        assert!(yaml.contains(&expected));
        assert!(!yaml.contains(SECRET_MARKER));
        // No ArgoCD tracking annotation ⇒ a sync never prunes it.
        assert!(!yaml.contains("annotations"));
        assert!(!yaml.contains("argocd.argoproj.io"));
    }

    // ---- negative --------------------------------------------------------

    #[test]
    fn negative_missing_required_file_names_the_key() {
        let dir = tempdir();
        let spec = one_secret("agent-forge", vec![key(&dir.join("absent"), false)]);
        let err = build_secret(
            &spec,
            "agent-seddon",
            std::slice::from_ref(&dir),
            DEFAULT_SIZE_CAP,
        )
        .expect_err("missing file must fail");
        match err {
            Error::MissingFile { key, .. } => assert_eq!(key, "token"),
            other => panic!("expected MissingFile, got {other:?}"),
        }
    }

    #[rstest]
    #[case::not_toml("this = = broken")]
    #[case::namespace_wrong_type("namespace = 7")]
    #[case::root_wrong_type("allowed_roots = \"/tmp\"")]
    #[case::secret_not_table("agent-forge = \"oops\"")]
    #[case::entry_bad_type("[agent-forge]\ntoken = 5")]
    #[case::table_entry_without_file("[agent-forge]\ntoken = { optional = true }")]
    fn negative_malformed_manifest_is_rejected(#[case] text: &str) {
        let err = parse_manifest(text).expect_err("malformed manifest must fail closed");
        assert!(matches!(err, Error::ManifestParse(_)), "got {err:?}");
    }

    // ---- boundary --------------------------------------------------------

    #[rstest]
    #[case::at_cap(8, 8, true)]
    #[case::under_cap(7, 8, true)]
    #[case::over_cap(9, 8, false)]
    fn boundary_size_cap(#[case] size: usize, #[case] cap: u64, #[case] accept: bool) {
        let dir = tempdir();
        let path = write_mode(&dir, "token", &vec![b'x'; size], 0o600);
        let spec = one_secret("agent-forge", vec![key(&path, false)]);
        let res = build_secret(&spec, "agent-seddon", std::slice::from_ref(&dir), cap);
        assert_eq!(res.is_ok(), accept, "size {size} vs cap {cap}: {res:?}");
        if !accept {
            assert!(matches!(res.unwrap_err(), Error::TooLarge { .. }));
        }
    }

    // ---- corner ----------------------------------------------------------

    #[test]
    fn corner_optional_absent_is_skipped_required_absent_errors() {
        let dir = tempdir();
        let present = write_mode(&dir, "signing.pem", b"KEY", 0o600);
        // optional + absent ⇒ skipped; the built Secret just omits it.
        let spec = one_secret(
            "agent-signing-key",
            vec![
                KeySpec {
                    key: "signing.pem".into(),
                    path: present,
                    optional: false,
                },
                KeySpec {
                    key: "previous.pem".into(),
                    path: dir.join("previous.pem"),
                    optional: true,
                },
            ],
        );
        let secret = build_secret(
            &spec,
            "agent-seddon",
            std::slice::from_ref(&dir),
            DEFAULT_SIZE_CAP,
        )
        .expect("optional-absent builds");
        assert!(secret.data.contains_key("signing.pem"));
        assert!(!secret.data.contains_key("previous.pem"));
    }

    #[test]
    fn corner_present_optional_file_is_still_validated() {
        let dir = tempdir();
        // optional, but present AND world-readable ⇒ still refused (optionality excuses
        // absence, not a bad file).
        let bad = write_mode(&dir, "previous.pem", b"KEY", 0o644);
        let spec = one_secret(
            "agent-signing-key",
            vec![KeySpec {
                key: "previous.pem".into(),
                path: bad,
                optional: true,
            }],
        );
        let err = build_secret(
            &spec,
            "agent-seddon",
            std::slice::from_ref(&dir),
            DEFAULT_SIZE_CAP,
        )
        .expect_err("present optional must still be validated");
        assert!(matches!(err, Error::TooPermissive { .. }), "got {err:?}");
    }

    // ---- adversarial (mandatory: untrusted-ish deploy input, must reject) -

    #[test]
    fn adversarial_group_or_world_readable_is_refused() {
        let dir = tempdir();
        for mode in [0o640u32, 0o604, 0o644, 0o666] {
            let path = write_mode(&dir, "token", SECRET_MARKER.as_bytes(), mode);
            let spec = one_secret("agent-forge", vec![key(&path, false)]);
            let err = build_secret(
                &spec,
                "agent-seddon",
                std::slice::from_ref(&dir),
                DEFAULT_SIZE_CAP,
            )
            .expect_err("permissive source must be refused");
            assert!(
                matches!(err, Error::TooPermissive { .. }),
                "mode {mode:o}: {err:?}"
            );
        }
    }

    #[test]
    fn adversarial_symlink_escaping_roots_is_refused() {
        let roots = tempdir();
        let outside = tempdir();
        // A real secret-looking target OUTSIDE the allowed roots (think /etc/shadow).
        let target = write_mode(&outside, "shadow", SECRET_MARKER.as_bytes(), 0o600);
        // A symlink INSIDE the roots that points at it.
        let link = roots.join("token");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        let spec = one_secret("agent-forge", vec![key(&link, false)]);
        let err = build_secret(
            &spec,
            "agent-seddon",
            std::slice::from_ref(&roots),
            DEFAULT_SIZE_CAP,
        )
        .expect_err("symlink escaping the roots must be refused");
        assert!(matches!(err, Error::OutsideRoots { .. }), "got {err:?}");
    }

    #[rstest]
    #[case::slash("a/b")]
    #[case::dotdot("..")]
    #[case::dot(".")]
    #[case::traversal("../../etc/shadow")]
    #[case::empty("")]
    #[case::space("a b")]
    fn adversarial_unsafe_key_is_refused(#[case] bad_key: &str) {
        let dir = tempdir();
        let path = write_mode(&dir, "src", b"KEY", 0o600);
        let spec = one_secret(
            "agent-forge",
            vec![KeySpec {
                key: bad_key.to_string(),
                path,
                optional: false,
            }],
        );
        let err = build_secret(
            &spec,
            "agent-seddon",
            std::slice::from_ref(&dir),
            DEFAULT_SIZE_CAP,
        )
        .expect_err("unsafe key must be refused");
        assert!(
            matches!(err, Error::UnsafeKey { .. }),
            "key {bad_key:?}: {err:?}"
        );
    }

    #[test]
    fn adversarial_no_secret_value_in_output() {
        let dir = tempdir();
        // Build every error variant we can from a marker-bearing source, plus the
        // dry-run summary, and assert the marker never appears.
        let mut messages = Vec::new();

        // TooPermissive (source contains the marker).
        let perm = write_mode(&dir, "perm", SECRET_MARKER.as_bytes(), 0o644);
        let e = build_secret(
            &one_secret("s", vec![key(&perm, false)]),
            "ns",
            std::slice::from_ref(&dir),
            DEFAULT_SIZE_CAP,
        )
        .unwrap_err();
        messages.push(e.to_string());

        // TooLarge (source contains the marker, cap below its size).
        let big = write_mode(&dir, "big", SECRET_MARKER.as_bytes(), 0o600);
        let e = build_secret(
            &one_secret("s", vec![key(&big, false)]),
            "ns",
            std::slice::from_ref(&dir),
            4,
        )
        .unwrap_err();
        messages.push(e.to_string());

        // The dry-run summary of a manifest whose Secret keys are present.
        let m = Manifest {
            namespace: None,
            allowed_roots: vec![dir.clone()],
            secrets: vec![one_secret("agent-forge", vec![key(&big, false)])],
        };
        messages.push(summary(&m, "ns"));

        for msg in &messages {
            assert!(
                !msg.contains(SECRET_MARKER),
                "a secret value leaked into output: {msg}"
            );
        }
    }

    #[test]
    fn adversarial_one_bad_file_aborts_the_batch() {
        let dir = tempdir();
        let good = write_mode(&dir, "good", b"KEY", 0o600);
        let bad = write_mode(&dir, "bad", b"KEY", 0o644);
        let m = Manifest {
            namespace: None,
            allowed_roots: vec![dir.clone()],
            secrets: vec![
                one_secret("a-good", vec![key(&good, false)]),
                one_secret("z-bad", vec![key(&bad, false)]),
            ],
        };
        // build_all is fail-closed: the permissive file fails the whole batch.
        assert!(build_all(&m, "ns", DEFAULT_SIZE_CAP).is_err());
    }
}
