//! Creating the GitHub App from a manifest (security-hardening 10, S26b): the
//! gate's `/auth/setup` and `/auth/setup/callback`.
//!
//! With no App there is no GitHub sign-in, so the first creation can't sit behind
//! one. Instead:
//! - At start, while no App exists, the gate writes a single-use setup code to
//!   [`SETUP_CODE_FILE`] in the setup directory (0600) and logs the file's path,
//!   never the code: logs leave the host (OTLP), the file doesn't. A code opens
//!   one page: its use writes the next code to the file (same deadline). Codes
//!   live [`SETUP_CODE_TTL_SECS`] from start; a restart writes a new one.
//! - `GET /auth/setup?code=…` with that code answers a page whose one button posts
//!   the App's manifest to GitHub. The manifest's URLs come from the front door's
//!   `main_origin`, never from the request. The page sets a setup cookie whose
//!   hash is kept with a fresh `state`, as the sign-in's login cookie is (F3).
//! - GitHub sends the browser back to `/auth/setup/callback?code=…&state=…`. The
//!   `state` is spent by its first use; the cookie must match it. The code is
//!   traded once (`POST /app-manifests/{code}/conversions`), and the reply,
//!   capped at [`MAX_CONVERSION_BYTES`] and shape-checked, goes straight to files
//!   in the setup directory (0600). None of it is logged or shown to the browser.
//! - Once [`APP_ID_FILE`] exists, both routes answer 404, and the setup code is
//!   gone. Rotating or recreating the App later is the operator's, not this.

use std::borrow::Cow;
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use agent_core::pending::{random_token, PendingTable};
use async_trait::async_trait;

use super::percent_decode;
use super::routes::{cookie_value, raw_param, Reply};
use crate::server::auth::{or_default, sha256_hex};

/// The page that posts the manifest.
pub const SETUP_PATH: &str = "/auth/setup";
/// Where GitHub sends the browser back with the manifest code.
pub const SETUP_CALLBACK_PATH: &str = "/auth/setup/callback";
/// The cookie binding a callback to the browser that opened the page.
pub const SETUP_COOKIE: &str = "__Host-seddon-setup";
/// How long a setup code written at start stays usable.
pub const SETUP_CODE_TTL_SECS: u64 = 3600;
/// How long one page's `state` (and its cookie) waits for GitHub's callback.
pub const SETUP_STATE_TTL_SECS: u64 = 600;
/// Pages open at once; more are refused until one lapses.
pub const MAX_SETUP_PENDING: usize = 16;
/// Largest conversion reply read; GitHub's is about 3 KiB, most of it the key.
pub const MAX_CONVERSION_BYTES: usize = 64 * 1024;
/// The conversion call's timeout.
pub const CONVERSION_TIMEOUT: Duration = Duration::from_secs(10);
/// Longest manifest code put in the conversion URL; GitHub's are 20 characters.
pub const MAX_MANIFEST_CODE: usize = 128;
pub const DEFAULT_GITHUB_URL: &str = "https://github.com";
pub const DEFAULT_API_URL: &str = "https://api.github.com";
/// Longest App name GitHub takes.
pub const MAX_APP_NAME: usize = 34;

/// The single-use setup code, while no App exists.
pub const SETUP_CODE_FILE: &str = "setup-code";
// The App's file names and shapes, shared with `[github_app] app_dir`'s read.
use agent_core::github_app_dir::{valid_app_slug, valid_client_id};
pub use agent_core::github_app_dir::{
    APP_ID_FILE, CLIENT_ID_FILE, CLIENT_SECRET_FILE, PRIVATE_KEY_FILE, SLUG_FILE,
    WEBHOOK_SECRET_FILE,
};

/// `[auth.gate.setup]`.
#[derive(Clone, Debug, Default)]
pub struct SetupConfig {
    /// Where the App's files go (absolute; made 0700 when missing).
    pub dir: PathBuf,
    /// GitHub's web origin; empty is `https://github.com`.
    pub github_url: String,
    /// GitHub's API base; empty is `https://api.github.com`.
    pub api_url: String,
    /// The organization that owns the App; empty is the operator's own account.
    pub org: String,
    /// The App's name on GitHub; empty is `agent-seddon`.
    pub app_name: String,
    /// Where GitHub delivers webhooks; empty asks for none.
    pub webhook_url: String,
}

/// Why a conversion call gave no reply to parse. None of these carry GitHub's
/// text: the browser sees a fixed message, the log the variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConversionError {
    Unreachable,
    Status(u16),
    TooLarge,
}

/// GitHub's `POST /app-manifests/{code}/conversions`, behind a trait so the
/// routes are tested without the network.
#[async_trait]
pub trait Conversion: Send + Sync {
    /// The reply body of a `201`, at most [`MAX_CONVERSION_BYTES`].
    async fn convert(&self, code: &str) -> Result<Vec<u8>, ConversionError>;
}

/// The real call, to `api_url`.
pub struct GithubConversion {
    http: reqwest::Client,
    api_url: String,
}

impl GithubConversion {
    pub fn new(api_url: &str) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(CONVERSION_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| format!("HTTP client: {e}"))?;
        Ok(Self {
            http,
            api_url: api_url.trim_end_matches('/').to_string(),
        })
    }
}

#[async_trait]
impl Conversion for GithubConversion {
    async fn convert(&self, code: &str) -> Result<Vec<u8>, ConversionError> {
        // `code` passed `manifest_code` (unreserved bytes only), so it is one
        // literal path segment.
        let url = format!("{}/app-manifests/{code}/conversions", self.api_url);
        let mut resp = self
            .http
            .post(url)
            .header("accept", "application/vnd.github+json")
            .header("x-github-api-version", "2022-11-28")
            .header("user-agent", "agent-seddon")
            .send()
            .await
            .map_err(|_| ConversionError::Unreachable)?;
        if resp.status().as_u16() != 201 {
            return Err(ConversionError::Status(resp.status().as_u16()));
        }
        if resp
            .content_length()
            .is_some_and(|n| n > MAX_CONVERSION_BYTES as u64)
        {
            return Err(ConversionError::TooLarge);
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|_| ConversionError::Unreachable)?
        {
            if body.len() + chunk.len() > MAX_CONVERSION_BYTES {
                return Err(ConversionError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

/// What the conversion created, shape-checked. `Debug` names the App, never its
/// secrets.
#[derive(Clone, PartialEq, Eq)]
pub struct CreatedApp {
    pub id: u64,
    pub slug: String,
    pub client_id: String,
    pub client_secret: String,
    pub webhook_secret: Option<String>,
    pub pem: String,
}

impl std::fmt::Debug for CreatedApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreatedApp")
            .field("id", &self.id)
            .field("slug", &self.slug)
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

/// The conversion reply's fields, or why it is refused. Every value is checked
/// before it reaches a file: an id, a slug and a client id of the shapes GitHub
/// issues, secrets of printable ASCII with no space, and a PEM private key.
pub fn parse_conversion(body: &[u8]) -> Result<CreatedApp, &'static str> {
    #[derive(serde::Deserialize)]
    struct Raw {
        id: u64,
        slug: String,
        client_id: String,
        client_secret: String,
        #[serde(default)]
        webhook_secret: Option<String>,
        pem: String,
    }
    if body.len() > MAX_CONVERSION_BYTES {
        return Err("conversion_too_large");
    }
    let raw: Raw = serde_json::from_slice(body).map_err(|_| "conversion_malformed")?;
    let secret = |s: &str, max: usize| {
        !s.is_empty() && s.len() <= max && s.bytes().all(|b| (0x21..=0x7e).contains(&b))
    };
    if raw.id == 0 {
        return Err("conversion_bad_id");
    }
    if !valid_app_slug(&raw.slug) {
        return Err("conversion_bad_slug");
    }
    if !valid_client_id(&raw.client_id) {
        return Err("conversion_bad_client_id");
    }
    if !secret(&raw.client_secret, 256) {
        return Err("conversion_bad_client_secret");
    }
    let webhook_secret = raw.webhook_secret.filter(|s| !s.is_empty());
    if webhook_secret.as_deref().is_some_and(|s| !secret(s, 256)) {
        return Err("conversion_bad_webhook_secret");
    }
    if !private_key_pem(&raw.pem) {
        return Err("conversion_bad_pem");
    }
    Ok(CreatedApp {
        id: raw.id,
        slug: raw.slug,
        client_id: raw.client_id,
        client_secret: raw.client_secret,
        webhook_secret,
        pem: raw.pem,
    })
}

/// One PEM private key block (PKCS#1 as GitHub issues it, or PKCS#8), ASCII,
/// at most 16 KiB.
fn private_key_pem(s: &str) -> bool {
    let body = s.trim_end();
    s.len() <= 16 * 1024
        && s.is_ascii()
        && [
            (
                "-----BEGIN RSA PRIVATE KEY-----",
                "-----END RSA PRIVATE KEY-----",
            ),
            ("-----BEGIN PRIVATE KEY-----", "-----END PRIVATE KEY-----"),
        ]
        .iter()
        .any(|(begin, end)| {
            body.starts_with(begin)
                && body.ends_with(end)
                && body.matches("-----BEGIN").count() == 1
        })
}

/// A manifest code GitHub sent back: unreserved URL bytes only, so it is one
/// literal path segment of the conversion URL.
fn manifest_code(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_MANIFEST_CODE
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~'))
        && s != "."
        && s != ".."
}

/// The setup code while it can still be used.
struct SetupCode {
    hash: String,
    expires_at: u64,
}

/// `/auth/setup` and its callback for one gate.
pub struct AppSetup {
    dir: PathBuf,
    github_url: String,
    org: String,
    app_name: String,
    /// The manifest the page posts, built once from `main_origin`.
    manifest: String,
    code: Mutex<Option<SetupCode>>,
    /// `state` → the setup cookie's hash.
    pending: PendingTable<String>,
    convert: Arc<dyn Conversion>,
    /// One conversion at a time, so two callbacks can't both write an App.
    converting: tokio::sync::Mutex<()>,
}

impl AppSetup {
    /// The setup for a front door at `main_origin` (canonical, no trailing `/`).
    /// While no App exists, a new setup code is written to the setup directory;
    /// `now` is when it starts to count down. Refused for a relative directory, a
    /// GitHub or webhook URL that isn't `https` (or `http` to a loopback IP), an
    /// organization or App name GitHub wouldn't take, or a directory that can't
    /// be made or written.
    pub fn new(
        cfg: &SetupConfig,
        main_origin: &str,
        now: u64,
        convert: Arc<dyn Conversion>,
    ) -> Result<Self, String> {
        let bad = |key: &str, why: String| format!("`[auth.gate.setup] {key}` {why}");
        if !cfg.dir.is_absolute() {
            return Err(bad(
                "dir",
                format!("`{}` must be absolute", cfg.dir.display()),
            ));
        }
        let github_url = or_default(&cfg.github_url, DEFAULT_GITHUB_URL);
        let api_url = or_default(&cfg.api_url, DEFAULT_API_URL);
        for (key, url) in [("github_url", &github_url), ("api_url", &api_url)] {
            check_base(url).map_err(|e| bad(key, format!("`{url}` {e}")))?;
        }
        let webhook_url = cfg.webhook_url.trim();
        if !webhook_url.is_empty() {
            check_base(webhook_url)
                .map_err(|e| bad("webhook_url", format!("`{webhook_url}` {e}")))?;
        }
        let org = cfg.org.trim().to_string();
        if !org.is_empty() && !org_name(&org) {
            return Err(bad(
                "org",
                format!("`{org}` is not a GitHub organization name"),
            ));
        }
        let app_name = or_default(&cfg.app_name, "agent-seddon");
        if app_name.len() > MAX_APP_NAME
            || !app_name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'-' | b'_' | b'.'))
        {
            return Err(bad(
                "app_name",
                format!(
                    "`{app_name}` must be at most {MAX_APP_NAME} letters, digits, spaces, `-`, `_` or `.`"
                ),
            ));
        }
        let setup = Self {
            dir: cfg.dir.clone(),
            manifest: manifest(main_origin, &app_name, webhook_url),
            github_url: github_url.trim_end_matches('/').to_string(),
            org,
            app_name,
            code: Mutex::new(None),
            pending: PendingTable::new(SETUP_STATE_TTL_SECS, MAX_SETUP_PENDING),
            convert,
            converting: tokio::sync::Mutex::new(()),
        };
        if setup.app_exists() {
            tracing::info!(dir = %setup.dir.display(), "a GitHub App exists; /auth/setup is closed");
            let _ = std::fs::remove_file(setup.dir.join(SETUP_CODE_FILE));
            return Ok(setup);
        }
        let code = random_token(32).ok_or("no system randomness for the setup code")?;
        private_dir(&setup.dir)
            .and_then(|()| write_private(&setup.dir, SETUP_CODE_FILE, &code))
            .map_err(|e| bad("dir", format!("`{}`: {e}", setup.dir.display())))?;
        *setup.code.lock().unwrap_or_else(PoisonError::into_inner) = Some(SetupCode {
            hash: sha256_hex(&code),
            expires_at: now.saturating_add(SETUP_CODE_TTL_SECS),
        });
        tracing::warn!(
            code_file = %setup.dir.join(SETUP_CODE_FILE).display(),
            open = %format!("{main_origin}{SETUP_PATH}?code=<the file's contents>"),
            expires_in_secs = SETUP_CODE_TTL_SECS,
            "no GitHub App yet: open the setup page with the code in the file to create one"
        );
        Ok(setup)
    }

    /// [`Self::new`] with the real conversion call, to `cfg.api_url`.
    pub fn github(cfg: &SetupConfig, main_origin: &str, now: u64) -> Result<Self, String> {
        let api = GithubConversion::new(&or_default(&cfg.api_url, DEFAULT_API_URL))?;
        Self::new(cfg, main_origin, now, Arc::new(api))
    }

    /// Whether the App's files are written ([`APP_ID_FILE`] exists).
    pub fn app_exists(&self) -> bool {
        self.dir.join(APP_ID_FILE).exists()
    }

    fn open(&self) -> bool {
        !self.app_exists()
            && self
                .code
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some()
    }

    /// `GET /auth/setup?code=…`: the page that posts the manifest, for the holder
    /// of the setup code.
    pub fn begin(&self, query: &str, now: u64) -> Reply {
        if !self.open() {
            return Reply::new(404, "not found");
        }
        let given = raw_param(query, "code")
            .ok()
            .flatten()
            .and_then(percent_decode)
            .map(|c| sha256_hex(&c));
        // The code is spent by its first use and the file gets a new one, under one
        // lock: a URL that a proxy or browser history kept opens nothing again.
        let rotated = {
            let mut code = self.code.lock().unwrap_or_else(PoisonError::into_inner);
            match (code.take(), &given) {
                (Some(c), Some(h)) if *h == c.hash && now <= c.expires_at => {
                    *code = random_token(32)
                        .filter(|next| write_private(&self.dir, SETUP_CODE_FILE, next).is_ok())
                        .map(|next| SetupCode {
                            hash: sha256_hex(&next),
                            expires_at: c.expires_at,
                        });
                    Some(code.is_some())
                }
                (kept, _) => {
                    *code = kept;
                    None
                }
            }
        };
        match rotated {
            None => {
                crate::server::audit::refused(SETUP_PATH, "setup_code_refused", Default::default());
                return Reply::new(
                    403,
                    "the setup code is wrong, used or expired; use the one in the setup code file, or restart the agent for a new one",
                );
            }
            Some(false) => {
                let _ = std::fs::remove_file(self.dir.join(SETUP_CODE_FILE));
                return Reply::new(
                    500,
                    "the next setup code can't be written; restart the agent",
                );
            }
            Some(true) => {}
        }
        let Some(cookie) = random_token(32) else {
            return Reply::new(503, "setup is unavailable");
        };
        let Ok((state, _)) = self.pending.insert(now, sha256_hex(&cookie)) else {
            return Reply::new(503, "too many setup pages are open; try again later");
        };
        let action = if self.org.is_empty() {
            format!("{}/settings/apps/new?state={state}", self.github_url)
        } else {
            format!(
                "{}/organizations/{}/settings/apps/new?state={state}",
                self.github_url, self.org
            )
        };
        let owner = if self.org.is_empty() {
            "your GitHub account".to_string()
        } else {
            format!("the <b>{}</b> organization", escape(&self.org))
        };
        let page = format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
             <title>Create the GitHub App</title></head><body>\
             <h1>Create the GitHub App</h1>\
             <p>This creates the GitHub App <b>{name}</b> on {owner}. GitHub shows its \
             settings before you confirm, and sends you back here when it is made.</p>\
             <form method=\"post\" action=\"{action}\">\
             <input type=\"hidden\" name=\"manifest\" value=\"{manifest}\">\
             <button type=\"submit\">Create on GitHub</button></form></body></html>",
            name = escape(&self.app_name),
            action = escape(&action),
            manifest = escape(&self.manifest),
        );
        Reply::html(200, page, &format!("form-action {}", self.github_url)).with(
            "set-cookie",
            super::routes::set_cookie(SETUP_COOKIE, &cookie, SETUP_STATE_TTL_SECS),
        )
    }

    /// `GET /auth/setup/callback?code=…&state=…`: trade GitHub's code for the App
    /// and write its files. The setup cookie is cleared whatever happens.
    pub async fn complete(&self, query: &str, cookie: Option<&str>, now: u64) -> Reply {
        if !self.open() {
            return Reply::new(404, "not found");
        }
        let clear = super::routes::clear_cookie(SETUP_COOKIE);
        let refused = |status: u16, reason: &'static str, body: &'static str| {
            crate::server::audit::refused(SETUP_CALLBACK_PATH, reason, Default::default());
            Reply::new(status, body).with("set-cookie", clear.clone())
        };
        let (Ok(Some(code_raw)), Ok(Some(state_raw))) =
            (raw_param(query, "code"), raw_param(query, "state"))
        else {
            return refused(403, "malformed_setup_callback", "setup refused");
        };
        let (Some(code), Some(state)) = (percent_decode(code_raw), percent_decode(state_raw))
        else {
            return refused(403, "malformed_setup_callback", "setup refused");
        };
        // Spent by its first use, right or wrong, before anything else is read.
        let Some(cookie_hash) = self.pending.take(now, &state) else {
            return refused(403, "setup_state_refused", "setup refused");
        };
        if !matches!(cookie_value(cookie, SETUP_COOKIE), Ok(Some(v)) if sha256_hex(v) == cookie_hash)
        {
            return refused(403, "setup_cookie_mismatch", "setup refused");
        }
        if !manifest_code(&code) {
            return refused(403, "malformed_setup_callback", "setup refused");
        }
        let _one = self.converting.lock().await;
        if self.app_exists() {
            return refused(
                409,
                "app_exists",
                "a GitHub App was already created here; delete the extra one on GitHub",
            );
        }
        let app = match self.convert.convert(&code).await {
            Ok(body) => parse_conversion(&body),
            Err(e) => {
                tracing::warn!(error = ?e, "the GitHub App conversion failed");
                Err("conversion_failed")
            }
        };
        let app = match app {
            Ok(app) => app,
            Err(reason) => {
                return refused(502, reason, "GitHub did not hand over the App; try again")
            }
        };
        if let Err(e) = write_app(&self.dir, &app) {
            tracing::error!(
                dir = %self.dir.display(),
                error = %e.kind(),
                "the GitHub App was created but its files could not be written"
            );
            return refused(
                500,
                "app_files_unwritable",
                "the App was created on GitHub but could not be saved; delete it there and restart",
            );
        }
        *self.code.lock().unwrap_or_else(PoisonError::into_inner) = None;
        let _ = std::fs::remove_file(self.dir.join(SETUP_CODE_FILE));
        tracing::info!(app_id = app.id, slug = %app.slug, "created the GitHub App from its manifest");
        let install = format!("{}/apps/{}/installations/new", self.github_url, app.slug);
        let page = format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
             <title>GitHub App created</title></head><body>\
             <h1>GitHub App created</h1>\
             <p>The App <b>{slug}</b> (id {id}) is saved. Sign-in with GitHub works once \
             Dex restarts with its client.</p>\
             <p>Next, <a href=\"{install}\">install it on repositories</a>.</p></body></html>",
            slug = escape(&app.slug),
            id = app.id,
            install = escape(&install),
        );
        Reply::html(200, page, "form-action 'none'").with("set-cookie", clear)
    }
}

/// The App's manifest (GitHub's App manifest flow): no webhook unless
/// `webhook_url` is set; GitHub returns to the portal's root after an install
/// (the first callback URL, forge-auth F5) and Dex signs people in with the
/// second; the permissions the fleet and the sign-in use.
fn manifest(main_origin: &str, name: &str, webhook_url: &str) -> String {
    let mut m = serde_json::json!({
        "name": name,
        "url": main_origin,
        "redirect_url": format!("{main_origin}{SETUP_CALLBACK_PATH}"),
        "callback_urls": [format!("{main_origin}/"), format!("{main_origin}/dex/callback")],
        "request_oauth_on_install": true,
        "public": false,
        "default_permissions": {
            "contents": "read",
            "metadata": "read",
            "pull_requests": "write",
            "members": "read",
            // The manifest takes GitHub's own resource names, not the REST API's
            // (`email_addresses`): an unknown one makes GitHub refuse the manifest.
            "emails": "read",
        },
        "default_events": [],
    });
    if !webhook_url.is_empty() {
        m["hook_attributes"] = serde_json::json!({ "url": webhook_url, "active": true });
        m["default_events"] = serde_json::json!(["pull_request"]);
    }
    m.to_string()
}

/// A base URL the gate posts to or names: `https` (or `http` to a loopback IP),
/// no credentials, query or fragment.
fn check_base(raw: &str) -> Result<(), String> {
    crate::server::auth::issuer::check_fetch_url(raw)?;
    let url = reqwest::Url::parse(raw).map_err(|e| format!("is not a valid URL ({e})"))?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err("must not carry a query or fragment".into());
    }
    Ok(())
}

/// A GitHub organization login: letters, digits and single `-`, at most 39.
fn org_name(s: &str) -> bool {
    s.len() <= 39
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// `s` safe in HTML text and in a double- or single-quoted attribute.
fn escape(s: &str) -> Cow<'_, str> {
    if !s
        .bytes()
        .any(|b| matches!(b, b'&' | b'<' | b'>' | b'"' | b'\''))
    {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// `dir`, made 0700 when it is missing.
fn private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

/// `value` into `dir/name`, mode 0600, by a rename so a reader never sees half.
fn write_private(dir: &Path, name: &str, value: &str) -> std::io::Result<()> {
    let tmp = dir.join(format!(".{name}.tmp"));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    // An old temp file keeps its mode through `open`.
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    f.write_all(value.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&tmp, dir.join(name))
}

/// The App's files, [`APP_ID_FILE`] last. A webhook secret file left from an
/// earlier App is removed when this one has none.
fn write_app(dir: &Path, app: &CreatedApp) -> std::io::Result<()> {
    private_dir(dir)?;
    write_private(dir, PRIVATE_KEY_FILE, &app.pem)?;
    write_private(dir, CLIENT_SECRET_FILE, &app.client_secret)?;
    match &app.webhook_secret {
        Some(s) => write_private(dir, WEBHOOK_SECRET_FILE, s)?,
        None => match std::fs::remove_file(dir.join(WEBHOOK_SECRET_FILE)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        },
    }
    write_private(dir, CLIENT_ID_FILE, &app.client_id)?;
    write_private(dir, SLUG_FILE, &app.slug)?;
    write_private(dir, APP_ID_FILE, &app.id.to_string())
}

#[cfg(test)]
mod tests;
