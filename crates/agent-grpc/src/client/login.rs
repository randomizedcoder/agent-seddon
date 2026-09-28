//! `agent login` for a person at a terminal (security-hardening S12,
//! docs/design/security-hardening/01-authentication.md "CLI").
//!
//! Three parts:
//!
//! - **The IdP side**, the Device Authorization Grant (RFC 8628). Discovery gives
//!   the device and token endpoints. The user approves the printed code in a
//!   browser, and polling the token endpoint then yields an ID token.
//! - **The agent side**: [`AgentAuth`] trades that ID token for an agent token
//!   (`AuthService.Exchange`). It also refreshes the token, logs out and asks
//!   who-am-I.
//! - **Between runs**: [`TokenFile`] keeps the agent token and its refresh handle
//!   in a `0600` file. [`LoginBearerSource`] hands the token to outbound seam calls
//!   and refreshes it in the background. The refresh handle rotates on every use, so
//!   the file is locked across a refresh. Two `agent` processes sharing one login
//!   then never both spend the same handle, which the server treats as theft and
//!   answers by revoking the session.
//!
//! The IdP is reached only at discovered URLs that pass [`check_fetch_url`]. Its
//! answers are size-capped, and its numbers (interval, lifetime) are clamped. Text
//! printed to the terminal (the user code, the verification URL) is refused if it
//! holds control characters, so a hostile answer cannot rewrite the screen.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use agent_core::{Bearer, BearerSource};
use agent_proto::pb;
use agent_proto::pb::auth_service_client::AuthServiceClient;
use agent_retry::RetryPolicy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tonic::transport::Channel;
use tonic::Code;

use super::service_token::{next_refresh, now_secs, usable_lifetime, REFRESH_SKEW_SECS};
use crate::server::check_fetch_url;
use crate::transport::Endpoint;

/// Largest IdP answer (discovery, device, token) read.
pub const MAX_IDP_BODY_BYTES: usize = 64 * 1024;
/// Largest token file read.
pub const MAX_TOKEN_FILE_BYTES: u64 = 64 * 1024;
/// RFC 8628 §3.2: the polling interval when the IdP names none.
pub const DEFAULT_INTERVAL_SECS: u64 = 5;
/// A longer interval from the IdP is capped, so a hostile answer cannot park the
/// login.
pub const MAX_INTERVAL_SECS: u64 = 60;
/// The longest a device code is waited on, whatever `expires_in` says.
pub const MAX_DEVICE_WINDOW_SECS: u64 = 1800;
/// Longest user code or verification URL shown.
pub const MAX_DISPLAY_CHARS: usize = 512;
/// The OAuth grant type for polling a device code.
pub const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// `client_kind` recorded on sessions opened by the CLI.
pub const CLI_CLIENT_KIND: &str = "cli";

// --- the IdP: device authorization grant ------------------------------------------

/// Where to start and poll a device login, from the issuer's discovery document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceEndpoints {
    pub device: String,
    pub token: String,
}

/// The IdP's OAuth client: `client_id` (the issuer's `audience`) and, for IdPs
/// that demand one even from a device client (Google), its secret.
#[derive(Clone)]
pub struct DeviceClient {
    pub client_id: String,
    pub client_secret: Option<String>,
}

impl std::fmt::Debug for DeviceClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceClient")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// What the user is asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevicePrompt {
    pub verification_uri: String,
    /// The same URL with the code filled in, when the IdP offers it.
    pub verification_uri_complete: Option<String>,
    pub user_code: String,
    /// How long the code is waited on, after clamping.
    pub expires_in: Duration,
}

/// Polling pace: the floor under the IdP's interval (so `interval: 0` cannot turn
/// the poll into a hammer) and the step a `slow_down` adds (RFC 8628 §3.5: 5 s).
#[derive(Clone, Copy, Debug)]
pub struct PollTiming {
    pub floor: Duration,
    pub slow_down_step: Duration,
}

impl Default for PollTiming {
    fn default() -> Self {
        Self {
            floor: Duration::from_secs(1),
            slow_down_step: Duration::from_secs(5),
        }
    }
}

/// One answer from the token endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PollAnswer {
    Pending,
    SlowDown,
    /// The ID token.
    Granted(String),
    /// The user refused.
    Denied,
    /// The device code lapsed.
    Expired,
    /// Anything else: a short reason, never the body.
    Failed(String),
}

/// An HTTP client for the IdP: every request bounded by `timeout`.
pub fn idp_client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| format!("HTTP client: {e}"))
}

/// Classify a token-endpoint answer (RFC 8628 §3.5).
pub fn classify_poll(success: bool, body: &Value) -> PollAnswer {
    if success {
        return match body.get("id_token").and_then(Value::as_str) {
            Some(t) if !t.is_empty() => PollAnswer::Granted(t.to_string()),
            _ => PollAnswer::Failed(
                "the issuer answered without an ID token (is `openid` in its scopes?)".into(),
            ),
        };
    }
    match body.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => PollAnswer::Pending,
        Some("slow_down") => PollAnswer::SlowDown,
        Some("access_denied") => PollAnswer::Denied,
        Some("expired_token") => PollAnswer::Expired,
        Some(e) if display_safe(e, 64) => {
            PollAnswer::Failed(format!("the issuer refused the device code ({e})"))
        }
        _ => PollAnswer::Failed("the issuer refused the device code".into()),
    }
}

/// The interval after `answer`: `slow_down` adds the step; the result stays
/// within `[floor, MAX_INTERVAL_SECS]`.
pub fn next_interval(current: Duration, answer: &PollAnswer, timing: PollTiming) -> Duration {
    let next = match answer {
        PollAnswer::SlowDown => current.saturating_add(timing.slow_down_step),
        _ => current,
    };
    next.clamp(timing.floor, Duration::from_secs(MAX_INTERVAL_SECS))
}

/// Printable, no control characters, bounded: safe to write to a terminal.
fn display_safe(s: &str, max: usize) -> bool {
    !s.is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
}

/// GET or POST and read a JSON body, capped. Errors are short classes, never the
/// URL or the body.
async fn fetch_json(req: reqwest::RequestBuilder, what: &str) -> Result<(bool, Value), String> {
    let mut resp = req.send().await.map_err(|e| {
        if e.is_timeout() {
            format!("{what}: timed out")
        } else if e.is_connect() {
            format!("{what}: could not connect")
        } else {
            format!("{what}: request failed")
        }
    })?;
    let success = resp.status().is_success();
    let status = resp.status().as_u16();
    let mut body = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|_| format!("{what}: the answer was cut off"))?
    {
        if body.len() + chunk.len() > MAX_IDP_BODY_BYTES {
            return Err(format!("{what}: the answer is over 64 KiB"));
        }
        body.extend_from_slice(&chunk);
    }
    match serde_json::from_slice(&body) {
        Ok(v) => Ok((success, v)),
        Err(_) if !success => Err(format!("{what}: HTTP {status}")),
        Err(_) => Err(format!("{what}: the answer is not JSON")),
    }
}

/// Read the issuer's discovery document for the device-flow endpoints. The
/// document must name `issuer` itself (a mix-up defence, as the verifier's
/// discovery does), and both endpoints must pass [`check_fetch_url`].
pub async fn discover_device(
    http: &reqwest::Client,
    issuer: &str,
) -> Result<DeviceEndpoints, String> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    check_fetch_url(&url).map_err(|e| format!("the issuer URL {e}"))?;
    let (ok, doc) = fetch_json(http.get(&url), "discovery").await?;
    if !ok {
        return Err("discovery: the issuer refused".into());
    }
    if doc.get("issuer").and_then(Value::as_str) != Some(issuer) {
        return Err("discovery names a different issuer".into());
    }
    let endpoint = |key: &str| -> Result<String, String> {
        let url = doc.get(key).and_then(Value::as_str).ok_or_else(|| {
            format!(
                "the issuer does not offer the device flow (discovery has no `{key}`); \
                 its OAuth client must be a \"TV and limited input\" / device client"
            )
        })?;
        check_fetch_url(url).map_err(|e| format!("discovery `{key}` {e}"))?;
        Ok(url.to_string())
    };
    Ok(DeviceEndpoints {
        device: endpoint("device_authorization_endpoint")?,
        token: endpoint("token_endpoint")?,
    })
}

/// Start a device login: ask for a code and check what will be shown.
async fn start_device(
    http: &reqwest::Client,
    endpoints: &DeviceEndpoints,
    client: &DeviceClient,
) -> Result<(DevicePrompt, String, Duration), String> {
    let mut form = vec![
        ("client_id", client.client_id.as_str()),
        ("scope", "openid email profile"),
    ];
    if let Some(secret) = &client.client_secret {
        form.push(("client_secret", secret.as_str()));
    }
    let (ok, body) = fetch_json(
        http.post(&endpoints.device).form(&form),
        "device authorization",
    )
    .await?;
    device_answer(ok, &body)
}

/// Check a device-authorization answer: the code to poll with, and only text that
/// is safe to print.
pub fn device_answer(ok: bool, body: &Value) -> Result<(DevicePrompt, String, Duration), String> {
    if !ok {
        return Err(match body.get("error").and_then(Value::as_str) {
            Some(e) if display_safe(e, 64) => format!("device authorization refused ({e})"),
            _ => "device authorization refused".into(),
        });
    }
    let text = |key: &str| body.get(key).and_then(Value::as_str);
    let device_code = text("device_code")
        .filter(|c| !c.is_empty() && c.len() <= MAX_DISPLAY_CHARS)
        .ok_or("device authorization: no usable `device_code`")?;
    let user_code = text("user_code")
        .filter(|c| display_safe(c, 64))
        .ok_or("device authorization: no printable `user_code`")?;
    let shown_url = |key: &str| -> Result<Option<String>, String> {
        match text(key) {
            None => Ok(None),
            Some(u) if display_safe(u, MAX_DISPLAY_CHARS) && check_fetch_url(u).is_ok() => {
                Ok(Some(u.to_string()))
            }
            Some(_) => Err(format!(
                "device authorization: `{key}` is not an https URL that is safe to show"
            )),
        }
    };
    // Google names it `verification_url` (pre-RFC); RFC 8628 says `verification_uri`.
    let verification_uri = match shown_url("verification_uri")? {
        Some(u) => u,
        None => {
            shown_url("verification_url")?.ok_or("device authorization: no `verification_uri`")?
        }
    };
    let verification_uri_complete = shown_url("verification_uri_complete")?;
    let secs = |key: &str, default: u64| body.get(key).and_then(Value::as_u64).unwrap_or(default);
    let expires_in = Duration::from_secs(secs("expires_in", 600).min(MAX_DEVICE_WINDOW_SECS));
    let interval = Duration::from_secs(secs("interval", DEFAULT_INTERVAL_SECS));
    Ok((
        DevicePrompt {
            verification_uri,
            verification_uri_complete,
            user_code: user_code.to_string(),
            expires_in,
        },
        device_code.to_string(),
        interval,
    ))
}

/// Run a device login to the end: show `prompt` the code, then poll until the
/// user approves (the ID token), refuses, or the code lapses.
pub async fn device_login(
    http: &reqwest::Client,
    endpoints: &DeviceEndpoints,
    client: &DeviceClient,
    timing: PollTiming,
    prompt: impl FnOnce(&DevicePrompt),
) -> Result<String, String> {
    let (shown, device_code, interval) = start_device(http, endpoints, client).await?;
    let deadline = tokio::time::Instant::now() + shown.expires_in;
    prompt(&shown);
    let mut interval = next_interval(interval, &PollAnswer::Pending, timing);
    loop {
        tokio::time::sleep(interval).await;
        if tokio::time::Instant::now() >= deadline {
            return Err("the login code expired before it was approved".into());
        }
        let mut form = vec![
            ("grant_type", DEVICE_GRANT),
            ("device_code", device_code.as_str()),
            ("client_id", client.client_id.as_str()),
        ];
        if let Some(secret) = &client.client_secret {
            form.push(("client_secret", secret.as_str()));
        }
        let answer = match fetch_json(http.post(&endpoints.token).form(&form), "token").await {
            Ok((ok, body)) => classify_poll(ok, &body),
            // A dropped poll is retried at the same pace until the deadline.
            Err(e) => {
                tracing::debug!(error = %e, "device login: poll failed, retrying");
                PollAnswer::Pending
            }
        };
        match answer {
            PollAnswer::Granted(id_token) => return Ok(id_token),
            PollAnswer::Denied => return Err("the login was refused at the issuer".into()),
            PollAnswer::Expired => {
                return Err("the login code expired before it was approved".into())
            }
            PollAnswer::Failed(reason) => return Err(reason),
            PollAnswer::Pending | PollAnswer::SlowDown => {
                interval = next_interval(interval, &answer, timing);
            }
        }
    }
}

// --- the agent: AuthService ---------------------------------------------------------

/// `AuthService` at one agent endpoint, over the process's client TLS.
#[derive(Clone)]
pub struct AgentAuth {
    endpoint: String,
    client: AuthServiceClient<Channel>,
}

/// `msg` with `authorization: Bearer <bearer>`; `None` when the token cannot be
/// a header value (a corrupted token file).
fn with_bearer<T>(msg: T, bearer: &str) -> Option<tonic::Request<T>> {
    let mut req = tonic::Request::new(msg);
    let value = format!("Bearer {bearer}").parse().ok()?;
    req.metadata_mut().insert("authorization", value);
    Some(req)
}

fn bad_token() -> tonic::Status {
    tonic::Status::unauthenticated("the stored token is not a valid header")
}

impl AgentAuth {
    /// Dial `endpoint` lazily (nothing is sent until the first call).
    pub fn connect(endpoint: &str) -> Result<Self, String> {
        let channel = Endpoint::parse(endpoint)
            .connect_lazy()
            .map_err(|e| format!("agent endpoint `{endpoint}`: {e}"))?;
        Ok(Self {
            endpoint: endpoint.to_string(),
            client: AuthServiceClient::new(channel),
        })
    }

    /// The endpoint this client dials.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Trade an IdP ID token for an agent token and a CLI session.
    pub async fn exchange(&self, id_token: &str) -> Result<pb::ExchangeResponse, tonic::Status> {
        let req = pb::ExchangeRequest {
            id_token: id_token.to_string(),
            client_kind: CLI_CLIENT_KIND.into(),
            ..Default::default()
        };
        Ok(self.client.clone().exchange(req).await?.into_inner())
    }

    /// A new token (and a new handle) from a live session.
    pub async fn refresh(&self, handle: &str) -> Result<pb::ExchangeResponse, tonic::Status> {
        let req = pb::RefreshRequest {
            refresh_handle: handle.to_string(),
        };
        Ok(self.client.clone().refresh(req).await?.into_inner())
    }

    /// End the session `bearer` belongs to; `true` when the server revoked it.
    pub async fn logout(&self, bearer: &str) -> Result<bool, tonic::Status> {
        let req = with_bearer(pb::LogoutRequest {}, bearer).ok_or_else(bad_token)?;
        Ok(self.client.clone().logout(req).await?.into_inner().revoked)
    }

    /// Who the server says `bearer` is.
    pub async fn who_am_i(&self, bearer: &str) -> Result<pb::WhoAmIResponse, tonic::Status> {
        let req = with_bearer(pb::WhoAmIRequest {}, bearer).ok_or_else(bad_token)?;
        Ok(self.client.clone().who_am_i(req).await?.into_inner())
    }
}

// --- between runs: the token file ---------------------------------------------------

/// A signed-in CLI session as kept on disk.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredLogin {
    /// The agent `AuthService` endpoint the session belongs to.
    pub endpoint: String,
    /// The login issuer's configured name.
    pub issuer: String,
    pub access_token: String,
    /// Unix seconds, after clamping.
    pub expires_at: u64,
    pub refresh_handle: String,
    /// When the session itself ends (no refresh past it).
    pub session_expires_at: u64,
}

impl std::fmt::Debug for StoredLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredLogin")
            .field("endpoint", &self.endpoint)
            .field("issuer", &self.issuer)
            .field("expires_at", &self.expires_at)
            .field("session_expires_at", &self.session_expires_at)
            .finish_non_exhaustive()
    }
}

impl StoredLogin {
    /// From an `Exchange` / `Refresh` answer at `now`. The server's `expires_at`
    /// is clamped like a service token's; an empty token or handle is refused.
    pub fn from_response(
        endpoint: &str,
        issuer: &str,
        resp: pb::ExchangeResponse,
        now: u64,
    ) -> Result<Self, String> {
        let lifetime = usable_lifetime(&resp.access_token, resp.expires_at, now)
            .ok_or("the agent returned no usable token")?;
        if resp.refresh_handle.is_empty() {
            return Err("the agent returned no refresh handle".into());
        }
        Ok(Self {
            endpoint: endpoint.to_string(),
            issuer: issuer.to_string(),
            access_token: resp.access_token,
            expires_at: now + lifetime,
            refresh_handle: resp.refresh_handle,
            session_expires_at: resp.session_expires_at,
        })
    }

    /// The token, unless it expires within the skew window at `now`.
    pub fn bearer_at(&self, now: u64) -> Option<Bearer> {
        (now.saturating_add(REFRESH_SKEW_SECS) < self.expires_at)
            .then(|| Bearer::new(self.access_token.clone()))
    }
}

/// `<dir>/<issuer>.json`, owner-only.
#[derive(Clone, Debug)]
pub struct TokenFile {
    path: PathBuf,
}

impl TokenFile {
    /// The file for `issuer` under `dir`. The name becomes a path segment, so it
    /// must be a plain identifier.
    pub fn in_dir(dir: &Path, issuer: &str) -> Result<Self, String> {
        if !agent_core::safe_segment(issuer) {
            return Err(format!("issuer name `{issuer}` is not a plain identifier"));
        }
        Ok(Self {
            path: dir.join(format!("{issuer}.json")),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The stored login; `None` when there is none. A file that others can read,
    /// is not a regular file, or is oversized is refused, not used.
    pub fn load(&self) -> Result<Option<StoredLogin>, String> {
        use std::os::unix::fs::PermissionsExt;
        let shown = self.path.display();
        let meta = match std::fs::symlink_metadata(&self.path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("`{shown}`: {e}")),
        };
        if !meta.file_type().is_file() {
            return Err(format!("`{shown}` is not a regular file"));
        }
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(format!(
                "`{shown}` is readable by others (mode {mode:o}); it holds a live token. \
                 Remove it and run `agent login` again"
            ));
        }
        if meta.len() > MAX_TOKEN_FILE_BYTES {
            return Err(format!("`{shown}` is over 64 KiB; not a token file"));
        }
        let mut text = String::new();
        std::fs::File::open(&self.path)
            .and_then(|f| f.take(MAX_TOKEN_FILE_BYTES).read_to_string(&mut text))
            .map_err(|e| format!("`{shown}`: {e}"))?;
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|_| format!("`{shown}` is not a stored login"))
    }

    /// Write atomically: an owner-only temporary file beside it, then a rename, so
    /// a reader sees the old login or the new one, never half of one.
    pub fn save(&self, login: &StoredLogin) -> Result<(), String> {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        let dir = self.path.parent().ok_or("token file has no directory")?;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| format!("`{}`: {e}", dir.display()))?;
        let tmp = self
            .path
            .with_extension(format!("json.{}.tmp", std::process::id()));
        let body = serde_json::to_vec_pretty(login).map_err(|e| e.to_string())?;
        let write = || -> std::io::Result<()> {
            let _ = std::fs::remove_file(&tmp);
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            f.write_all(&body)?;
            f.sync_all()?;
            std::fs::rename(&tmp, &self.path)
        };
        write().map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("`{}`: {e}", self.path.display())
        })
    }

    /// Delete the stored login; `true` when there was one.
    pub fn remove(&self) -> Result<bool, String> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(format!("`{}`: {e}", self.path.display())),
        }
    }

    /// Hold `<file>.lock` exclusively until the returned handle drops.
    fn lock_blocking(&self) -> Result<std::fs::File, String> {
        use std::os::unix::fs::OpenOptionsExt;
        let path = self.path.with_extension("json.lock");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(|e| format!("`{}`: {e}", path.display()))?;
        file.lock()
            .map_err(|e| format!("`{}`: {e}", path.display()))?;
        Ok(file)
    }

    async fn lock(&self) -> Result<std::fs::File, String> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.lock_blocking())
            .await
            .map_err(|e| e.to_string())?
    }
}

/// A refresh that did not produce a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshError {
    /// The session is over (revoked, expired, handle refused): sign in again.
    Ended(String),
    /// Worth retrying (the agent was unreachable, the file was busy).
    Transient(String),
}

/// Refresh the login in `file` under its lock and return the fresh one. When
/// another process already refreshed (the file holds a newer usable token), that
/// one is adopted and no handle is spent.
pub async fn refresh_stored(
    file: &TokenFile,
    auth: &AgentAuth,
    seen: Option<&StoredLogin>,
) -> Result<StoredLogin, RefreshError> {
    let _lock = file.lock().await.map_err(RefreshError::Transient)?;
    let on_disk = file
        .load()
        .map_err(RefreshError::Ended)?
        .ok_or_else(|| RefreshError::Ended("signed out (the token file is gone)".into()))?;
    let now = now_secs();
    let newer = seen.is_none_or(|s| on_disk.expires_at > s.expires_at);
    if newer && on_disk.bearer_at(now).is_some() {
        return Ok(on_disk);
    }
    let resp = match auth.refresh(&on_disk.refresh_handle).await {
        Ok(r) => r,
        Err(s) => {
            return Err(match s.code() {
                Code::Unauthenticated | Code::PermissionDenied | Code::InvalidArgument => {
                    RefreshError::Ended("the agent ended this login session".into())
                }
                code => RefreshError::Transient(format!("refresh failed ({code:?})")),
            })
        }
    };
    let fresh = StoredLogin::from_response(&on_disk.endpoint, &on_disk.issuer, resp, now)
        .map_err(RefreshError::Transient)?;
    file.save(&fresh).map_err(RefreshError::Transient)?;
    Ok(fresh)
}

/// The signed-in user's token for outbound seam calls, kept fresh from the token
/// file. Installed as the process [`BearerSource`] by `[grpc.client] bearer =
/// "login"`.
pub struct LoginBearerSource {
    file: TokenFile,
    auth: AgentAuth,
    current: RwLock<Option<StoredLogin>>,
}

impl LoginBearerSource {
    /// Load the stored login; refused when there is none. Call inside a Tokio
    /// runtime (the agent channel is dialled lazily on it).
    pub fn open(file: TokenFile) -> Result<Self, String> {
        let login = file.load()?.ok_or_else(|| {
            format!(
                "not signed in (no `{}`): run `agent login`",
                file.path().display()
            )
        })?;
        let auth = AgentAuth::connect(&login.endpoint)?;
        Ok(Self {
            file,
            auth,
            current: RwLock::new(Some(login)),
        })
    }

    fn snapshot(&self) -> Option<StoredLogin> {
        self.current
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn set(&self, login: Option<StoredLogin>) {
        *self
            .current
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = login;
    }

    /// Refresh once; on success, how long until the next refresh.
    pub async fn refresh(&self) -> Result<Duration, RefreshError> {
        let seen = self.snapshot();
        match refresh_stored(&self.file, &self.auth, seen.as_ref()).await {
            Ok(fresh) => {
                let wait = next_refresh(fresh.expires_at.saturating_sub(now_secs()));
                self.set(Some(fresh));
                Ok(wait)
            }
            Err(RefreshError::Ended(reason)) => {
                self.set(None);
                Err(RefreshError::Ended(reason))
            }
            Err(e) => Err(e),
        }
    }

    /// Keep the token fresh: refresh at two thirds of each lifetime, back off after
    /// a transient failure, and stop (clearing the token) once the session ends.
    pub fn spawn_refresher(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let backoff = RetryPolicy::new(u32::MAX)
            .with_base_delay(Duration::from_secs(1))
            .with_max_delay(Duration::from_secs(60));
        // unscoped-spawn: the process's own credential; there is no caller to carry.
        tokio::spawn(async move {
            let mut failures: u32 = 0;
            let mut wait = self
                .snapshot()
                .map(|l| next_refresh(l.expires_at.saturating_sub(now_secs())))
                .unwrap_or_default();
            loop {
                tokio::time::sleep(wait).await;
                wait = match self.refresh().await {
                    Ok(next) => {
                        failures = 0;
                        next
                    }
                    Err(RefreshError::Ended(reason)) => {
                        tracing::warn!(%reason, "login: session ended; run `agent login`");
                        return;
                    }
                    Err(RefreshError::Transient(e)) => {
                        tracing::warn!(error = %e, "login: refresh failed, retrying");
                        let w = backoff.ceiling(failures);
                        failures = failures.saturating_add(1);
                        w
                    }
                };
            }
        })
    }
}

impl BearerSource for LoginBearerSource {
    fn bearer(&self) -> Option<Bearer> {
        self.snapshot().and_then(|l| l.bearer_at(now_secs()))
    }
}

#[cfg(test)]
mod tests;
