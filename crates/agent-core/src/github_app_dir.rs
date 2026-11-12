//! The GitHub App's files, as the gate's `/auth/setup` writes them
//! (security-hardening S26b): one directory, one value per file, all 0600 to the
//! agent's user. The gate writes them; `[github_app] app_dir` and the NixOS module's
//! Dex read them. The names, the shapes and the read live here so both sides agree.
//!
//! Only the ids are read into memory. The secrets stay files, named by `file:`
//! references, so they are read where they are used and never pass through config.

use std::io::Read as _;
use std::path::Path;

/// Written last: its presence is what "an App exists" means.
pub const APP_ID_FILE: &str = "app-id";
pub const SLUG_FILE: &str = "slug";
pub const CLIENT_ID_FILE: &str = "client-id";
pub const CLIENT_SECRET_FILE: &str = "client-secret";
pub const PRIVATE_KEY_FILE: &str = "private-key.pem";
/// Absent when the manifest asked for no webhook.
pub const WEBHOOK_SECRET_FILE: &str = "webhook-secret";

/// Longest id file read; the values are at most 100 bytes.
const MAX_ID_FILE: u64 = 256;

/// A GitHub App slug as GitHub issues one: lowercase letters, digits and `-`, not
/// starting with `-`, at most 100 bytes.
pub fn valid_app_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && !s.starts_with('-')
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// An OAuth client id as GitHub issues one (`Iv23li…`): letters, digits and `.`,
/// at most 64 bytes.
pub fn valid_client_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
}

/// A GitHub App id: a positive decimal number.
pub fn valid_app_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 20
        && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<u64>().is_ok_and(|n| n > 0)
}

/// The App's public ids, and which optional secret file is there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppIds {
    pub app_id: String,
    pub slug: String,
    pub client_id: String,
    pub has_webhook_secret: bool,
}

/// The App in `dir`: `Ok(None)` while no App exists ([`APP_ID_FILE`] missing), an
/// error when one exists but a file is missing, too large or of the wrong shape.
/// Errors name the file, never its contents.
pub fn read(dir: &Path) -> Result<Option<AppIds>, String> {
    let small = |name: &str| -> Result<Option<String>, String> {
        let path = dir.join(name);
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("`{}`: {e}", path.display())),
        };
        let mut s = String::new();
        file.take(MAX_ID_FILE + 1)
            .read_to_string(&mut s)
            .map_err(|e| format!("`{}`: {e}", path.display()))?;
        if s.len() as u64 > MAX_ID_FILE {
            return Err(format!("`{}` is too large", path.display()));
        }
        // An operator's editor may add a newline; the gate writes none.
        Ok(Some(s.trim_end_matches(['\n', '\r']).to_string()))
    };
    let Some(app_id) = small(APP_ID_FILE)? else {
        return Ok(None);
    };
    let need = |name: &str, ok: fn(&str) -> bool| -> Result<String, String> {
        let v = small(name)?.ok_or_else(|| format!("`{}` is missing", dir.join(name).display()))?;
        if ok(&v) {
            Ok(v)
        } else {
            Err(format!("`{}` is malformed", dir.join(name).display()))
        }
    };
    if !valid_app_id(&app_id) {
        return Err(format!(
            "`{}` is malformed",
            dir.join(APP_ID_FILE).display()
        ));
    }
    let slug = need(SLUG_FILE, valid_app_slug)?;
    let client_id = need(CLIENT_ID_FILE, valid_client_id)?;
    for name in [CLIENT_SECRET_FILE, PRIVATE_KEY_FILE] {
        if !dir.join(name).is_file() {
            return Err(format!("`{}` is missing", dir.join(name).display()));
        }
    }
    Ok(Some(AppIds {
        app_id,
        slug,
        client_id,
        has_webhook_secret: dir.join(WEBHOOK_SECRET_FILE).is_file(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// A fresh directory per case (agent-core can't use agent-testkit, which needs it).
    fn scratch() -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("agent-core-appdir-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn app(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        for (name, value) in [
            (SLUG_FILE, "agent-seddon-l2"),
            (CLIENT_ID_FILE, "Iv23li.abc123"),
            (CLIENT_SECRET_FILE, "0123456789abcdef"),
            (
                PRIVATE_KEY_FILE,
                "-----BEGIN RSA PRIVATE KEY-----\nMIIE\n-----END RSA PRIVATE KEY-----\n",
            ),
            (APP_ID_FILE, "4242"),
        ] {
            std::fs::write(dir.join(name), value).unwrap();
        }
    }

    fn ids(webhook: bool) -> AppIds {
        AppIds {
            app_id: "4242".into(),
            slug: "agent-seddon-l2".into(),
            client_id: "Iv23li.abc123".into(),
            has_webhook_secret: webhook,
        }
    }

    #[rstest]
    #[case::positive_whole_app(|_d: &Path| {}, Ok(Some(ids(false))))]
    #[case::positive_with_webhook(|d: &Path| std::fs::write(d.join(WEBHOOK_SECRET_FILE), "w").unwrap(), Ok(Some(ids(true))))]
    #[case::corner_no_app_yet(|d: &Path| std::fs::remove_file(d.join(APP_ID_FILE)).unwrap(), Ok(None))]
    #[case::corner_no_dir(|d: &Path| std::fs::remove_dir_all(d).unwrap(), Ok(None))]
    #[case::corner_trailing_newline(|d: &Path| std::fs::write(d.join(APP_ID_FILE), "4242\n").unwrap(), Ok(Some(ids(false))))]
    #[case::negative_missing_slug(|d: &Path| std::fs::remove_file(d.join(SLUG_FILE)).unwrap(), Err("slug` is missing"))]
    #[case::negative_missing_key(|d: &Path| std::fs::remove_file(d.join(PRIVATE_KEY_FILE)).unwrap(), Err("private-key.pem` is missing"))]
    #[case::negative_missing_client_secret(|d: &Path| std::fs::remove_file(d.join(CLIENT_SECRET_FILE)).unwrap(), Err("client-secret` is missing"))]
    #[case::negative_app_id_zero(|d: &Path| std::fs::write(d.join(APP_ID_FILE), "0").unwrap(), Err("app-id` is malformed"))]
    #[case::adversarial_app_id_injection(|d: &Path| std::fs::write(d.join(APP_ID_FILE), "1\"\nx = \"y").unwrap(), Err("app-id` is malformed"))]
    #[case::adversarial_slug_traversal(|d: &Path| std::fs::write(d.join(SLUG_FILE), "../../x").unwrap(), Err("slug` is malformed"))]
    #[case::adversarial_client_id_space(|d: &Path| std::fs::write(d.join(CLIENT_ID_FILE), "Iv1 x").unwrap(), Err("client-id` is malformed"))]
    #[case::boundary_id_file_at_cap(|d: &Path| std::fs::write(d.join(SLUG_FILE), "a".repeat(MAX_ID_FILE as usize)).unwrap(), Err("slug` is malformed"))]
    #[case::adversarial_huge_id_file(|d: &Path| std::fs::write(d.join(SLUG_FILE), "a".repeat(1 << 20)).unwrap(), Err("slug` is too large"))]
    #[case::adversarial_key_is_a_dir(|d: &Path| { std::fs::remove_file(d.join(PRIVATE_KEY_FILE)).unwrap(); std::fs::create_dir(d.join(PRIVATE_KEY_FILE)).unwrap(); }, Err("private-key.pem` is missing"))]
    fn read_cases(#[case] tweak: fn(&Path), #[case] want: Result<Option<AppIds>, &str>) {
        let dir = scratch().join("github-app");
        app(&dir);
        tweak(&dir);
        match (read(&dir), want) {
            (Ok(got), Ok(want)) => assert_eq!(got, want),
            (Err(e), Err(frag)) => {
                assert!(e.contains(frag), "{e}");
                for secret in ["0123456789abcdef", "MIIE", "\"y"] {
                    assert!(!e.contains(secret), "no contents in {e}");
                }
            }
            (got, want) => panic!("got {got:?}, want {want:?}"),
        }
    }

    #[rstest]
    #[case::positive("4242", true)]
    #[case::boundary_u64_max("18446744073709551615", true)]
    #[case::boundary_overflow("18446744073709551616", false)]
    #[case::negative_zero("0", false)]
    #[case::negative_empty("", false)]
    #[case::adversarial_sign("+1", false)]
    #[case::adversarial_space(" 1", false)]
    fn valid_app_id_cases(#[case] s: &str, #[case] want: bool) {
        assert_eq!(valid_app_id(s), want);
    }
}
