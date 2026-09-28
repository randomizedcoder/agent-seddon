//! `agent login` / `logout` / `whoami`, and the process credential `[grpc.client]
//! bearer` installs (security-hardening S12,
//! docs/design/security-hardening/01-authentication.md "CLI").
//!
//! The protocol pieces live in `agent_grpc::client::login`. This module maps
//! config onto them: which issuer (`[[auth.issuers]]` + `--issuer`), which agent
//! (`[grpc.client] auth_endpoint` + `--endpoint`), and where the login is kept
//! (`$XDG_CONFIG_HOME/agent-seddon/tokens/<issuer>.json`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_grpc::client::login::{
    device_login, discover_device, idp_client, refresh_stored, AgentAuth, DeviceClient,
    DevicePrompt, LoginBearerSource, PollTiming, RefreshError, StoredLogin, TokenFile,
};
use agent_grpc::server::ResolvedIssuer;
use anyhow::Context;

use crate::config::{ClientBearer, Config};

/// How long one IdP request may take.
const IDP_TIMEOUT: Duration = Duration::from_secs(20);

/// Where logins are kept: `$XDG_CONFIG_HOME/agent-seddon/tokens`, else
/// `$HOME/.config/agent-seddon/tokens`. A relative `XDG_CONFIG_HOME` is ignored,
/// as the XDG spec says.
pub fn token_dir() -> anyhow::Result<PathBuf> {
    token_dir_from(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

fn token_dir_from(xdg: Option<PathBuf>, home: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let base = match (
        xdg.filter(|p| p.is_absolute()),
        home.filter(|p| p.is_absolute()),
    ) {
        (Some(xdg), _) => xdg,
        (None, Some(home)) => home.join(".config"),
        (None, None) => anyhow::bail!("neither XDG_CONFIG_HOME nor HOME is an absolute path"),
    };
    Ok(base.join("agent-seddon").join("tokens"))
}

/// The token file for the login issuer `wanted` names (or the only one).
fn token_file(cfg: &Config, wanted: Option<&str>) -> anyhow::Result<TokenFile> {
    let name = cfg.auth.login_issuer(wanted).map_err(anyhow::Error::msg)?;
    TokenFile::in_dir(&token_dir()?, name).map_err(anyhow::Error::msg)
}

/// A login issuer's device-flow settings: its discovery URL, client and secret.
struct LoginIssuer {
    name: String,
    issuer: String,
    client: DeviceClient,
}

fn login_issuer(cfg: &Config, wanted: Option<&str>) -> anyhow::Result<LoginIssuer> {
    let name = cfg.auth.login_issuer(wanted).map_err(anyhow::Error::msg)?;
    let params = crate::auth_params::login_issuers(&cfg.auth)
        .into_iter()
        .find(|p| p.name == name)
        .context("login issuer vanished")?;
    let resolved = ResolvedIssuer::resolve(&params).map_err(anyhow::Error::msg)?;
    // Google's profile accepts `iss` with and without the scheme; discovery needs
    // the URL. Anything else must name exactly one issuer (Entra: one tenant).
    let urls: Vec<&String> = resolved
        .accepted_iss
        .iter()
        .filter(|i| i.starts_with("https://") || i.starts_with("http://"))
        .collect();
    let [issuer] = urls.as_slice() else {
        anyhow::bail!(
            "login issuer `{name}` accepts several `iss` values; set its `issuer` to the one \
             `agent login` should use"
        );
    };
    let secret_ref = cfg
        .auth
        .issuers
        .iter()
        .find(|i| i.name.trim() == name)
        .map(|i| i.client_secret.as_str())
        .unwrap_or_default();
    let secret = crate::secrets::resolve(crate::secrets::SecretScope::Operator, secret_ref)
        .map_err(|e| anyhow::anyhow!("login issuer `{name}` client_secret: {e}"))?;
    Ok(LoginIssuer {
        name: name.to_string(),
        issuer: (*issuer).clone(),
        client: DeviceClient {
            client_id: resolved.audience.clone(),
            client_secret: (!secret.expose().is_empty()).then(|| secret.expose().to_string()),
        },
    })
}

/// Print what the user must do, on stderr (stdout stays for results).
fn show_prompt(p: &DevicePrompt) {
    eprintln!();
    match &p.verification_uri_complete {
        Some(url) => eprintln!("  Open {url}"),
        None => eprintln!("  Open {}", p.verification_uri),
    }
    eprintln!("  and confirm the code:  {}", p.user_code);
    eprintln!(
        "  (waiting up to {} min for the approval)",
        p.expires_in.as_secs().div_ceil(60)
    );
    eprintln!();
}

/// `agent login [--issuer NAME] [--endpoint ADDR]`: device flow at the issuer,
/// `Exchange` at the agent, then keep the agent token in the token file.
pub async fn login(
    cfg: &Config,
    issuer: Option<&str>,
    endpoint: Option<&str>,
) -> anyhow::Result<()> {
    crate::builder::install_client_tls(&cfg.grpc.tls.client)?;
    let idp = login_issuer(cfg, issuer)?;
    let endpoint = endpoint
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .or_else(|| Some(cfg.grpc.client.auth_endpoint.trim()).filter(|e| !e.is_empty()))
        .context(
            "no agent to sign in at: set `[grpc.client] auth_endpoint` or pass `--endpoint`",
        )?;
    if !crate::config::private_or_tls_endpoint(endpoint) {
        anyhow::bail!(
            "`{endpoint}` must be `https://…`, a loopback IP or `unix:` (an ID token is sent on it)"
        );
    }
    let http = idp_client(IDP_TIMEOUT).map_err(anyhow::Error::msg)?;
    let endpoints = discover_device(&http, &idp.issuer)
        .await
        .map_err(|e| anyhow::anyhow!("login issuer `{}`: {e}", idp.name))?;
    let id_token = device_login(
        &http,
        &endpoints,
        &idp.client,
        PollTiming::default(),
        show_prompt,
    )
    .await
    .map_err(|e| anyhow::anyhow!("login issuer `{}`: {e}", idp.name))?;
    let auth = AgentAuth::connect(endpoint).map_err(anyhow::Error::msg)?;
    let resp = auth
        .exchange(&id_token)
        .await
        .map_err(|s| anyhow::anyhow!("the agent refused the login: {}", s.message()))?;
    let login = StoredLogin::from_response(endpoint, &idp.name, resp, now_secs())
        .map_err(anyhow::Error::msg)?;
    let file = TokenFile::in_dir(&token_dir()?, &idp.name).map_err(anyhow::Error::msg)?;
    file.save(&login).map_err(anyhow::Error::msg)?;
    let me = auth
        .who_am_i(&login.access_token)
        .await
        .map_err(|s| anyhow::anyhow!("signed in, but WhoAmI failed: {}", s.message()))?;
    println!(
        "signed in as {} in tenant {} (roles: {}); saved to {}",
        shown_subject(&me.email, &me.subject),
        me.tenant,
        me.roles.join(", "),
        file.path().display()
    );
    Ok(())
}

/// The stored login with a usable token, refreshing it when stale.
async fn usable_login(file: &TokenFile) -> anyhow::Result<(StoredLogin, AgentAuth)> {
    let stored = file
        .load()
        .map_err(anyhow::Error::msg)?
        .with_context(|| format!("not signed in (no `{}`)", file.path().display()))?;
    let auth = AgentAuth::connect(&stored.endpoint).map_err(anyhow::Error::msg)?;
    if stored.bearer_at(now_secs()).is_some() {
        return Ok((stored, auth));
    }
    let fresh = refresh_stored(file, &auth, Some(&stored))
        .await
        .map_err(|e| match e {
            RefreshError::Ended(r) => anyhow::anyhow!("{r}: run `agent login`"),
            RefreshError::Transient(r) => anyhow::anyhow!("{r}"),
        })?;
    Ok((fresh, auth))
}

/// `agent whoami [--issuer NAME]`.
pub async fn whoami(cfg: &Config, issuer: Option<&str>) -> anyhow::Result<()> {
    crate::builder::install_client_tls(&cfg.grpc.tls.client)?;
    let file = token_file(cfg, issuer)?;
    let (login, auth) = usable_login(&file).await?;
    let me = auth
        .who_am_i(&login.access_token)
        .await
        .map_err(|s| anyhow::anyhow!("WhoAmI: {}", s.message()))?;
    println!("subject      {}", shown_subject(&me.email, &me.subject));
    println!("tenant       {}", me.tenant);
    println!("issuer       {}", me.issuer);
    println!("roles        {}", me.roles.join(", "));
    println!("permissions  {}", me.permissions.join(", "));
    println!("session      {}", me.sid);
    println!("agent        {}", login.endpoint);
    Ok(())
}

/// `agent logout [--issuer NAME]`: revoke the session at the agent, then forget
/// it here. The local file goes even when the agent cannot be reached, and the
/// error says the session is still live there until it expires.
pub async fn logout(cfg: &Config, issuer: Option<&str>) -> anyhow::Result<()> {
    crate::builder::install_client_tls(&cfg.grpc.tls.client)?;
    let file = token_file(cfg, issuer)?;
    let revoked = match usable_login(&file).await {
        Ok((login, auth)) => auth
            .logout(&login.access_token)
            .await
            .map_err(|s| anyhow::anyhow!("{}", s.message())),
        Err(e) => Err(e),
    };
    let removed = file.remove().map_err(anyhow::Error::msg)?;
    match (revoked, removed) {
        (Ok(_), _) => {
            println!("signed out; the session is revoked");
            Ok(())
        }
        (Err(_), false) => {
            println!("not signed in");
            Ok(())
        }
        (Err(e), true) => Err(e.context(
            "removed the local login, but the agent did not revoke the session \
             (it stays usable until it expires)",
        )),
    }
}

/// The email when there is one, else the subject.
fn shown_subject<'a>(email: &'a str, subject: &'a str) -> &'a str {
    if email.is_empty() {
        subject
    } else {
        email
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Install `[grpc.client] bearer` as this process's credential. Once per process:
/// a second agent built in the same process keeps the first source.
pub(crate) fn install_client_bearer(cfg: &Config) -> anyhow::Result<()> {
    let source: Arc<dyn agent_core::BearerSource> =
        match cfg.grpc.client.bearer_kind().map_err(anyhow::Error::msg)? {
            ClientBearer::None => return Ok(()),
            ClientBearer::Login(wanted) => {
                let source = Arc::new(
                    LoginBearerSource::open(token_file(cfg, wanted)?)
                        .map_err(anyhow::Error::msg)?,
                );
                if agent_core::install_bearer_source(source.clone()) {
                    source.spawn_refresher();
                    tracing::info!("outbound calls carry the stored `agent login` token");
                }
                return Ok(());
            }
            ClientBearer::Ref(r) => {
                let token = crate::secrets::resolve(crate::secrets::SecretScope::Operator, r)
                    .map_err(|e| anyhow::anyhow!("`[grpc.client] bearer`: {e}"))?;
                if token.expose().is_empty() {
                    anyhow::bail!("`[grpc.client] bearer` resolves to nothing");
                }
                Arc::new(agent_core::StaticBearer(agent_core::Bearer::new(
                    token.expose().to_string(),
                )))
            }
        };
    if !agent_core::install_bearer_source(source) {
        tracing::debug!("a bearer source is already installed in this process");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
