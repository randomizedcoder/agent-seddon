//! `agent-egress` — an opt-in, fail-closed **egress allow-list** for the agent's own
//! process (multi-tenancy C23-3c, docs/design/multi-tenancy/01-process-isolation.md).
//!
//! The `bwrap` sandbox already gives *reviewed code* zero egress (`--unshare-net`), but
//! the agent process itself makes outbound HTTP with `reqwest` (LLM providers, git forge
//! REST, MCP, web-fetch/search, …) to any host. A prompt-injected or misconfigured flow
//! can therefore reach an unintended host. This crate closes that with a tiny loopback
//! **CONNECT filtering proxy**: the runtime pins the process's `reqwest` egress to it via
//! `HTTPS_PROXY`/`HTTP_PROXY` (which every `reqwest` client honors by default), and the
//! proxy only lets connections through to allow-listed hosts.
//!
//! ## Trust model (stated plainly)
//! This is a **policy boundary for the trusted agent process** and the model-driven
//! `reqwest` paths — enforced cooperatively via the proxy-env pin. It is **not** a hard
//! kernel boundary against a fully-compromised process (which could unset the env); that
//! is what the `bwrap` netns gives *reviewed* code. It also does not cover non-`reqwest`
//! egress (tonic OTLP / internal gRPC, the ClickHouse native protocol, or the `git`
//! subprocess) — those talk to the operator's own trusted backends. A kernel-level
//! all-egress netns is a possible later hardening.
//!
//! ## Fail-closed
//! An empty allow-list refuses *everything*; a non-allow-listed or malformed CONNECT
//! target is refused (`403`); the caller (the runtime) refuses to start the agent if the
//! listener cannot bind. The `host` a rule matches is attacker-influenceable (a
//! `web_fetch` URL, an MCP endpoint), so matching is exact/suffix with no substring
//! escape — see [`HostMatcher`].

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Cap on the request head we buffer before deciding allow/deny. A well-formed proxy
/// request line + headers is tiny; anything past this is refused (a slow-loris / oversize
/// guard). Bytes beyond the head belong to the tunnel body and are forwarded, not parsed.
const MAX_HEAD_BYTES: usize = 8 * 1024;

/// How long the client has to send a complete request head before we give up. The tunnel
/// itself (after approval) is not time-bounded — model responses stream for minutes.
const HEAD_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// One entry in the egress allow-list. Built by the runtime from config
/// (`agent_runtime::derive_egress_allowlist`) and matched against the CONNECT/absolute-form
/// target host. All hosts are normalised to lowercase, sans a trailing FQDN dot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRule {
    /// Match this exact host (case-insensitive).
    Exact(String),
    /// Match this host or any subdomain of it — parsed from a `.example.com` or
    /// `*.example.com` rule. Stored **without** the leading dot (e.g. `example.com`).
    Suffix(String),
}

impl HostRule {
    /// Parse one allow-list entry. Returns `None` for anything that is not a plausible
    /// host rule (empty, contains a path/userinfo/whitespace) — fail-closed: a junk entry
    /// simply does not widen the allow-list.
    pub fn parse(raw: &str) -> Option<Self> {
        let s = raw.trim().to_ascii_lowercase();
        // A bare host rule never carries a scheme, path, userinfo or whitespace. Reject
        // rather than try to sanitise (a `//`, `@` or space means the caller passed
        // something other than a host — do not guess a host out of it).
        let (kind_suffix, body) = if let Some(rest) = s.strip_prefix("*.") {
            (true, rest)
        } else if let Some(rest) = s.strip_prefix('.') {
            (true, rest)
        } else {
            (false, s.as_str())
        };
        let body = body.trim_end_matches('.'); // tolerate a trailing FQDN dot
        if body.is_empty()
            || body.contains('/')
            || body.contains('@')
            || body.contains(':')
            || body.contains(char::is_whitespace)
            || body.contains('*')
        {
            return None;
        }
        Some(if kind_suffix {
            HostRule::Suffix(body.to_string())
        } else {
            HostRule::Exact(body.to_string())
        })
    }
}

/// The egress allow-list. `allows(host)` is the single decision point the proxy consults.
#[derive(Debug, Clone, Default)]
pub struct HostMatcher {
    rules: Vec<HostRule>,
}

impl HostMatcher {
    /// Build a matcher from pre-parsed rules. An **empty** matcher allows nothing
    /// (maximally fail-closed): enabling egress with no derivable hosts blocks all egress.
    pub fn new(rules: Vec<HostRule>) -> Self {
        Self { rules }
    }

    /// Build a matcher directly from raw config strings, dropping unparseable entries.
    pub fn from_strings<I, S>(raw: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::new(
            raw.into_iter()
                .filter_map(|s| HostRule::parse(s.as_ref()))
                .collect(),
        )
    }

    /// Does the allow-list permit egress to `host`? `host` is the authority host with no
    /// port or userinfo (the proxy strips those before calling). Matching is
    /// case-insensitive, FQDN-dot-insensitive, and — crucially — has **no substring
    /// escape**: `Suffix("example.com")` matches `example.com` and `*.example.com`, never
    /// `example.com.evil.net` or `notexample.com`.
    pub fn allows(&self, host: &str) -> bool {
        let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
        // A clean authority host has none of these; if one slipped through, refuse.
        if h.is_empty() || h.contains('/') || h.contains('@') || h.contains(char::is_whitespace) {
            return false;
        }
        self.rules.iter().any(|r| match r {
            HostRule::Exact(e) => h == *e,
            HostRule::Suffix(sfx) => h == *sfx || h.ends_with(&format!(".{sfx}")),
        })
    }

    /// Number of rules (for logging/tests).
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the allow-list is empty (⇒ blocks all egress).
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// The environment variables to export so the process's `reqwest` clients route through
/// the loopback proxy at `addr`. Both upper- and lower-case forms are set (different libs
/// read different cases); `NO_PROXY` keeps loopback/internal-seam traffic direct.
pub fn proxy_env(addr: SocketAddr) -> Vec<(String, String)> {
    let url = format!("http://{addr}");
    let no_proxy = "localhost,127.0.0.1,::1".to_string();
    vec![
        ("HTTP_PROXY".into(), url.clone()),
        ("HTTPS_PROXY".into(), url.clone()),
        ("http_proxy".into(), url.clone()),
        ("https_proxy".into(), url),
        ("NO_PROXY".into(), no_proxy.clone()),
        ("no_proxy".into(), no_proxy),
    ]
}

/// Accept loop: serve the filtering proxy on `listener` until the process exits. Each
/// connection is handled on its own task; a handler error is logged and the connection
/// dropped (never propagated — one bad client must not stop the proxy).
pub async fn serve(listener: TcpListener, matcher: Arc<HostMatcher>) {
    loop {
        let (client, peer) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "egress proxy accept failed");
                continue;
            }
        };
        let matcher = matcher.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(client, &matcher).await {
                tracing::debug!(%peer, error = %e, "egress proxy connection closed");
            }
        });
    }
}

/// Parsed first line of a proxy request: the method and the target authority host+port.
struct Target {
    /// `true` for `CONNECT` (tunnel); `false` for an absolute-form HTTP request.
    connect: bool,
    host: String,
    /// `host:port` to dial upstream.
    authority: String,
}

async fn handle_conn(mut client: TcpStream, matcher: &HostMatcher) -> std::io::Result<()> {
    // Read the request head (up to the blank line) with a cap + timeout. `rest` is any
    // bytes past the head already in flight (a pipelined body / early ClientHello).
    let (head, rest) = match tokio::time::timeout(HEAD_READ_TIMEOUT, read_head(&mut client)).await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            let _ = client
                .write_all(b"HTTP/1.1 408 Request Timeout\r\n\r\n")
                .await;
            return Ok(());
        }
    };

    let target = match parse_target(&head) {
        Some(t) => t,
        None => {
            let _ = client.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
            return Ok(());
        }
    };

    // The security decision. Fail-closed on anything not allow-listed.
    if !matcher.allows(&target.host) {
        tracing::info!(host = %target.host, "egress refused (not allow-listed)");
        let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await;
        return Ok(());
    }

    let mut upstream = match TcpStream::connect(&target.authority).await {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(authority = %target.authority, error = %e, "egress upstream dial failed");
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await;
            return Ok(());
        }
    };

    if target.connect {
        // Tunnel: acknowledge, replay any early bytes, then splice both directions.
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        if !rest.is_empty() {
            upstream.write_all(&rest).await?;
        }
    } else {
        // Absolute-form HTTP: RFC 7230 §5.3.2 requires origin servers to accept the
        // absolute request-target, so we forward the head verbatim (no rewrite) plus any
        // buffered body, then splice.
        upstream.write_all(&head).await?;
        if !rest.is_empty() {
            upstream.write_all(&rest).await?;
        }
    }

    // Carry the connection until either side closes. Not time-bounded (streamed model
    // responses are long-lived); errors here are ordinary connection teardown.
    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
    Ok(())
}

/// Read from `client` until the end of the request head (`\r\n\r\n`) or the cap. Returns
/// `(head_including_terminator, bytes_read_past_the_head)`.
async fn read_head(client: &mut TcpStream) -> std::io::Result<(Vec<u8>, Vec<u8>)> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(end) = find_head_end(&buf) {
            let rest = buf.split_off(end);
            return Ok((buf, rest));
        }
        if buf.len() > MAX_HEAD_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "request head exceeds cap",
            ));
        }
        let n = client.read(&mut chunk).await?;
        if n == 0 {
            // EOF before a complete head.
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed before request head",
            ));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Byte index just past the `\r\n\r\n` that ends the head, if present.
fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

/// Parse the first request line into a [`Target`]. Rejects anything that is not a clean
/// `CONNECT host:port` or an absolute-form `METHOD http[s]://host[:port]/path` line.
fn parse_target(head: &[u8]) -> Option<Target> {
    // Only the first line matters; it must be valid UTF-8 and free of control chars
    // (a CRLF-injection guard — the line was split on the real CRLF already).
    let first = head.split(|&b| b == b'\r' || b == b'\n').next()?;
    let line = std::str::from_utf8(first).ok()?;
    if line.chars().any(char::is_control) {
        return None;
    }
    let mut it = line.split(' ');
    let method = it.next()?;
    let raw_target = it.next()?;
    let _version = it.next()?;
    if it.next().is_some() {
        return None; // more than 3 space-separated tokens ⇒ malformed
    }

    if method.eq_ignore_ascii_case("CONNECT") {
        // Authority-form: host:port (no scheme, no path).
        let (host, port) = split_authority(raw_target)?;
        return Some(Target {
            authority: format!("{host}:{port}"),
            connect: true,
            host,
        });
    }

    // Absolute-form: scheme://host[:port]/path — only http/https make sense here.
    let after_scheme = raw_target
        .strip_prefix("http://")
        .map(|r| (r, 80u16))
        .or_else(|| raw_target.strip_prefix("https://").map(|r| (r, 443u16)))?;
    let (rest, default_port) = after_scheme;
    // Authority ends at the first '/', '?' or '#'.
    let authority_part = rest
        .split(['/', '?', '#'])
        .next()
        .filter(|a| !a.is_empty())?;
    if authority_part.contains('@') {
        return None; // userinfo not permitted — refuse rather than strip
    }
    let (host, port) = split_host_port(authority_part, default_port)?;
    Some(Target {
        authority: format!("{host}:{port}"),
        connect: false,
        host,
    })
}

/// Split a CONNECT `host:port` authority into `(host, port)`. Port is mandatory for
/// CONNECT (proxies require it).
fn split_authority(auth: &str) -> Option<(String, u16)> {
    if auth.contains('@') || auth.contains('/') {
        return None;
    }
    let (host, port_str) = auth.rsplit_once(':')?;
    if host.is_empty() {
        return None;
    }
    let port: u16 = port_str.parse().ok()?;
    Some((host.to_ascii_lowercase(), port))
}

/// Split an absolute-form authority `host[:port]` into `(lowercased host, port)`,
/// defaulting the port when absent.
fn split_host_port(auth: &str, default_port: u16) -> Option<(String, u16)> {
    match auth.rsplit_once(':') {
        Some((host, port_str)) if !host.is_empty() => {
            let port: u16 = port_str.parse().ok()?;
            Some((host.to_ascii_lowercase(), port))
        }
        Some(_) => None, // empty host before ':'
        None => Some((auth.to_ascii_lowercase(), default_port)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ---- HostMatcher (pure) ------------------------------------------------

    fn matcher(rules: &[&str]) -> HostMatcher {
        HostMatcher::from_strings(rules.iter().copied())
    }

    #[rstest]
    #[case::exact("api.anthropic.com", "api.anthropic.com", true)]
    #[case::exact_other_host("api.anthropic.com", "api.openai.com", false)]
    #[case::suffix_dot(".github.com", "codeload.github.com", true)]
    #[case::suffix_star("*.githubusercontent.com", "objects.githubusercontent.com", true)]
    #[case::suffix_apex(".github.com", "github.com", true)]
    fn positive_matches(#[case] rule: &str, #[case] host: &str, #[case] want: bool) {
        assert_eq!(matcher(&[rule]).allows(host), want);
    }

    #[rstest]
    #[case::mixed_case("api.GitHub.com")]
    #[case::trailing_dot("api.github.com.")]
    fn corner_case_insensitive_and_fqdn_dot(#[case] host: &str) {
        assert!(matcher(&["api.github.com"]).allows(host));
    }

    #[rstest]
    #[case::with_port_stripped_by_caller("api.github.com")]
    fn positive_host_without_port(#[case] host: &str) {
        // The proxy passes a bare host (no port); allows() must accept it.
        assert!(matcher(&["api.github.com"]).allows(host));
    }

    #[test]
    fn negative_unlisted_refused() {
        assert!(!matcher(&["api.github.com"]).allows("evil.example"));
    }

    #[test]
    fn negative_empty_allowlist_refuses_all() {
        let m = HostMatcher::new(vec![]);
        assert!(m.is_empty());
        assert!(!m.allows("api.github.com"));
        assert!(!m.allows("localhost"));
    }

    #[test]
    fn boundary_max_len_host_matches_itself() {
        let label = "a".repeat(63);
        let host = format!("{label}.{label}.example.com");
        assert!(matcher(&[".example.com"]).allows(&host));
    }

    // Mandatory adversarial cases — the target host is attacker-influenceable.
    #[rstest]
    // A registered-suffix trick must NOT match an exact rule.
    #[case::exact_substring_suffix("api.anthropic.com", "api.anthropic.com.evil.net", false)]
    // A suffix rule must not match a host that merely *ends with* the string without the
    // dot boundary, nor a look-alike registrable domain.
    #[case::suffix_no_dot_boundary(".githubusercontent.com", "evilgithubusercontent.com", false)]
    #[case::suffix_not_bare_substring(".github.com", "github.com.evil.net", false)]
    fn adversarial_substring_and_suffix_not_matched(
        #[case] rule: &str,
        #[case] host: &str,
        #[case] want: bool,
    ) {
        assert_eq!(matcher(&[rule]).allows(host), want);
    }

    #[test]
    fn adversarial_userinfo_and_path_hosts_refused() {
        let m = matcher(&["api.github.com"]);
        assert!(!m.allows("user@api.github.com"));
        assert!(!m.allows("api.github.com/../evil"));
        assert!(!m.allows("api.github.com evil.com"));
    }

    #[test]
    fn adversarial_unparseable_rules_dropped_not_widened() {
        // Junk rules must not create an allow-everything hole.
        let m = matcher(&["", "  ", "http://x/y", "a b", "user@h", "*", "*.*"]);
        assert!(m.is_empty(), "no junk rule should parse into a live rule");
        assert!(!m.allows("anything.example"));
    }

    #[rstest]
    #[case::exact("api.github.com", HostRule::Exact("api.github.com".into()))]
    #[case::dot_suffix(".github.com", HostRule::Suffix("github.com".into()))]
    #[case::star_suffix("*.github.com", HostRule::Suffix("github.com".into()))]
    #[case::upper("API.GitHub.COM", HostRule::Exact("api.github.com".into()))]
    #[case::trailing_dot("api.github.com.", HostRule::Exact("api.github.com".into()))]
    fn positive_rule_parse(#[case] raw: &str, #[case] want: HostRule) {
        assert_eq!(HostRule::parse(raw), Some(want));
    }

    // ---- proxy_env ---------------------------------------------------------

    #[test]
    fn positive_proxy_env_sets_both_cases_and_no_proxy() {
        let addr: SocketAddr = "127.0.0.1:54321".parse().unwrap();
        let env = proxy_env(addr);
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(
            get("HTTPS_PROXY").as_deref(),
            Some("http://127.0.0.1:54321")
        );
        assert_eq!(
            get("https_proxy").as_deref(),
            Some("http://127.0.0.1:54321")
        );
        assert_eq!(get("HTTP_PROXY").as_deref(), Some("http://127.0.0.1:54321"));
        assert!(get("NO_PROXY").unwrap().contains("127.0.0.1"));
    }

    // ---- parse_target (pure) ----------------------------------------------

    #[test]
    fn positive_parse_connect_and_absolute() {
        let c = parse_target(b"CONNECT api.github.com:443 HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        assert!(c.connect);
        assert_eq!(c.host, "api.github.com");
        assert_eq!(c.authority, "api.github.com:443");

        let h = parse_target(b"GET http://llama.local:8095/v1/models HTTP/1.1\r\n\r\n").unwrap();
        assert!(!h.connect);
        assert_eq!(h.host, "llama.local");
        assert_eq!(h.authority, "llama.local:8095");

        let d = parse_target(b"GET http://example.com/path HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(d.authority, "example.com:80"); // default http port
    }

    #[rstest]
    #[case::empty(b"" as &[u8])]
    #[case::garbage(b"not a request line at all\r\n\r\n")]
    #[case::connect_no_port(b"CONNECT api.github.com HTTP/1.1\r\n\r\n")]
    #[case::too_many_tokens(b"GET http://x/ HTTP/1.1 extra\r\n\r\n")]
    #[case::userinfo(b"GET http://user@evil.com/ HTTP/1.1\r\n\r\n")]
    #[case::control_chars(b"CONNECT api.github.com:443\x00 HTTP/1.1\r\n\r\n")]
    fn adversarial_malformed_target_rejected(#[case] head: &[u8]) {
        assert!(parse_target(head).is_none());
    }

    // ---- proxy behaviour (loopback integration; runs in the hermetic gate) -

    /// Spawn the proxy on an ephemeral loopback port; return its address.
    async fn spawn_proxy(m: HostMatcher) -> SocketAddr {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve(listener, Arc::new(m)));
        addr
    }

    /// Spawn a trivial upstream TCP server that, after any bytes arrive, replies with a
    /// fixed line and closes. Returns its address.
    async fn spawn_upstream(reply: &'static [u8]) -> SocketAddr {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut s, _)) = listener.accept().await {
                let mut b = [0u8; 512];
                let _ = s.read(&mut b).await;
                let _ = s.write_all(reply).await;
                let _ = s.shutdown().await;
            }
        });
        addr
    }

    #[tokio::test]
    async fn positive_connect_to_allowed_host_tunnels() {
        let up = spawn_upstream(b"PONG").await;
        // Allow the upstream by its loopback host.
        let proxy = spawn_proxy(matcher(&["127.0.0.1"])).await;
        let mut c = TcpStream::connect(proxy).await.unwrap();
        c.write_all(
            format!(
                "CONNECT 127.0.0.1:{} HTTP/1.1\r\nHost: x\r\n\r\n",
                up.port()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let mut resp = [0u8; 64];
        let n = c.read(&mut resp).await.unwrap();
        assert!(
            std::str::from_utf8(&resp[..n])
                .unwrap()
                .starts_with("HTTP/1.1 200"),
            "expected 200 tunnel established"
        );
        // Now the tunnel is live: send bytes, get the upstream's reply back.
        c.write_all(b"ping").await.unwrap();
        let mut back = Vec::new();
        let _ = c.read_to_end(&mut back).await;
        assert_eq!(&back, b"PONG");
    }

    #[tokio::test]
    async fn negative_connect_to_disallowed_returns_403() {
        let up = spawn_upstream(b"PONG").await;
        let proxy = spawn_proxy(matcher(&["api.github.com"])).await; // 127.0.0.1 NOT allowed
        let mut c = TcpStream::connect(proxy).await.unwrap();
        c.write_all(format!("CONNECT 127.0.0.1:{} HTTP/1.1\r\n\r\n", up.port()).as_bytes())
            .await
            .unwrap();
        let mut resp = Vec::new();
        let _ = c.read_to_end(&mut resp).await;
        assert!(
            std::str::from_utf8(&resp)
                .unwrap()
                .starts_with("HTTP/1.1 403"),
            "expected 403 for a non-allow-listed host"
        );
    }

    #[tokio::test]
    async fn positive_http_absolute_form_allowed_forwards() {
        let up = spawn_upstream(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await;
        let proxy = spawn_proxy(matcher(&["127.0.0.1"])).await;
        let mut c = TcpStream::connect(proxy).await.unwrap();
        c.write_all(
            format!(
                "GET http://127.0.0.1:{}/x HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                up.port()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let mut resp = Vec::new();
        let _ = c.read_to_end(&mut resp).await;
        assert!(std::str::from_utf8(&resp)
            .unwrap()
            .starts_with("HTTP/1.1 200 OK"));
    }

    #[tokio::test]
    async fn adversarial_malformed_request_line_no_panic_400() {
        let proxy = spawn_proxy(matcher(&["127.0.0.1"])).await;
        let mut c = TcpStream::connect(proxy).await.unwrap();
        c.write_all(b"GARBAGE\r\n\r\n").await.unwrap();
        let mut resp = Vec::new();
        let _ = c.read_to_end(&mut resp).await;
        assert!(std::str::from_utf8(&resp)
            .unwrap()
            .starts_with("HTTP/1.1 400"));
    }

    #[tokio::test]
    async fn adversarial_oversized_head_closed() {
        let proxy = spawn_proxy(matcher(&["127.0.0.1"])).await;
        let mut c = TcpStream::connect(proxy).await.unwrap();
        // Never send the terminating blank line; flood past the cap.
        let big = vec![b'A'; MAX_HEAD_BYTES + 1024];
        // Write may error once the peer drops us — that is the expected outcome.
        let _ = c.write_all(&big).await;
        let mut resp = Vec::new();
        // The connection is closed with no valid response (fail-closed).
        let _ = c.read_to_end(&mut resp).await;
        assert!(
            resp.is_empty()
                || !std::str::from_utf8(&resp)
                    .unwrap_or("")
                    .starts_with("HTTP/1.1 200"),
            "an oversized head must never yield a tunnel"
        );
    }
}
