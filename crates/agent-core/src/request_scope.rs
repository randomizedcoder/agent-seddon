//! The inbound bearer token and the one helper that re-installs a request's
//! ambient scope (security-hardening S5,
//! docs/design/security-hardening/04-service-integration.md).
//!
//! A served request carries four task-locals: the routing identity
//! ([`crate::AGENT_IDENTITY`]), the verified principal ([`crate::AGENT_PRINCIPAL`]),
//! the verbatim agent token it arrived with ([`AGENT_BEARER`]), and how many agent
//! services it has passed through ([`AGENT_HOPS`]). A downstream seam call forwards
//! the token on the caller's behalf and the hop count one higher (S9). A spawned task
//! inherits none of them, so work handed to `tokio::spawn` on behalf of a request
//! captures [`RequestScope::current`] and runs under [`scope_request`].
//!
//! Work with no caller (a scheduler job, a background reindex) has no inbound token;
//! its outbound calls carry this process's own service token instead, from the
//! process-global [`BearerSource`] ([`outbound_bearer`]).

use std::fmt;
use std::future::Future;
use std::sync::{Arc, OnceLock};

use crate::{current_identity, current_principal, SessionKey, VerifiedPrincipal};

/// An agent-issued bearer token. A credential: `Debug` never prints it, and the
/// value is reachable only through [`Bearer::expose`].
#[derive(Clone, PartialEq, Eq)]
pub struct Bearer(Arc<str>);

impl Bearer {
    pub fn new(token: impl Into<Arc<str>>) -> Self {
        Self(token.into())
    }

    /// The raw token, for an `authorization` header and nothing else.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Bearer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Bearer(<redacted>)")
    }
}

tokio::task_local! {
    /// The agent token the current request was authenticated with, installed by the
    /// auth layer beside [`crate::AGENT_PRINCIPAL`]. Unset when auth is disabled,
    /// outside a served handler, and in a spawned task not run under
    /// [`scope_request`].
    pub static AGENT_BEARER: Bearer;

    /// How many agent services the current request has passed through, this one
    /// included (the first server a client reaches is hop 1). Installed by the auth
    /// layer from the inbound `x-agent-hops`, which it never trusts to go down: the
    /// server always adds its own hop.
    pub static AGENT_HOPS: u8;
}

/// The current request's bearer token, or `None` when none is in scope.
pub fn current_bearer() -> Option<Bearer> {
    AGENT_BEARER.try_with(Clone::clone).ok()
}

/// The current request's hop count, or `0` outside a served request.
pub fn current_hops() -> u8 {
    AGENT_HOPS.try_with(|h| *h).unwrap_or(0)
}

/// Where this process's **own** agent token comes from: the credential its outbound
/// seam calls carry when no caller's token is in scope. A service token obtained over
/// mTLS (S10) or the CLI's stored login (S12) implement it; `None` means the process
/// has none and such calls go out unauthenticated, as before.
pub trait BearerSource: Send + Sync {
    fn bearer(&self) -> Option<Bearer>;
}

/// A fixed token (tests, and a token handed over at startup).
#[derive(Debug, Clone)]
pub struct StaticBearer(pub Bearer);

impl BearerSource for StaticBearer {
    fn bearer(&self) -> Option<Bearer> {
        Some(self.0.clone())
    }
}

static SERVICE_BEARER: OnceLock<Arc<dyn BearerSource>> = OnceLock::new();

/// Install the process's service token source. Once per process: a second install
/// is refused (returns `false`) so no later caller can swap the credential.
pub fn install_bearer_source(source: Arc<dyn BearerSource>) -> bool {
    SERVICE_BEARER.set(source).is_ok()
}

/// The process's own token, when a [`BearerSource`] is installed and has one.
pub fn service_bearer() -> Option<Bearer> {
    SERVICE_BEARER.get().and_then(|s| s.bearer())
}

/// Which token an outbound seam call carries: the caller's, forwarded on their
/// behalf, when one is in scope; otherwise the process's own. A caller's token is
/// never replaced by the service's, so a user's call is never upgraded to the
/// service's authority.
pub fn select_bearer(forwarded: Option<Bearer>, service: Option<Bearer>) -> Option<Bearer> {
    forwarded.or(service)
}

/// [`select_bearer`] over the ambient scope and the installed [`BearerSource`].
pub fn outbound_bearer() -> Option<Bearer> {
    select_bearer(current_bearer(), service_bearer())
}

/// A request's ambient scope: what [`scope_request`] re-installs.
#[derive(Clone, Debug, Default)]
pub struct RequestScope {
    pub identity: Option<SessionKey>,
    pub principal: Option<VerifiedPrincipal>,
    pub bearer: Option<Bearer>,
    /// The hop count; `0` installs nothing.
    pub hops: u8,
}

impl RequestScope {
    /// Everything in scope for the current task, to carry across a `spawn`.
    pub fn current() -> Self {
        Self {
            identity: current_identity(),
            principal: current_principal(),
            bearer: current_bearer(),
            hops: current_hops(),
        }
    }
}

/// Run `fut` with every part of `scope` that is set installed as the ambient
/// identity, principal, bearer and hop count. Parts that are `None` (or a hop count
/// of `0`) stay unset (they are not inherited from an enclosing scope's absence, and
/// nothing is invented).
pub async fn scope_request<F: Future>(scope: RequestScope, fut: F) -> F::Output {
    let RequestScope {
        identity,
        principal,
        bearer,
        hops,
    } = scope;
    let fut = async move {
        match hops {
            0 => fut.await,
            n => AGENT_HOPS.scope(n, fut).await,
        }
    };
    let fut = async move {
        match bearer {
            Some(b) => AGENT_BEARER.scope(b, fut).await,
            None => fut.await,
        }
    };
    let fut = async move {
        match principal {
            Some(p) => crate::principal_scope(p, fut).await,
            None => fut.await,
        }
    };
    match identity {
        Some(i) => crate::scope(i, fut).await,
        None => fut.await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn principal() -> VerifiedPrincipal {
        VerifiedPrincipal {
            tenant: "acme".into(),
            subject: "user:google/1".into(),
            roles: vec!["reader".into()],
        }
    }

    fn full() -> RequestScope {
        RequestScope {
            identity: Some(SessionKey::parse("acme", "s1").unwrap()),
            principal: Some(principal()),
            bearer: Some(Bearer::new("tok")),
            hops: 2,
        }
    }

    #[rstest]
    // desc: every part installed.
    #[case::positive_all_parts(full(), true, true, true, 2)]
    // desc: only the bearer (a service token with no routing identity).
    #[case::corner_bearer_only(RequestScope { bearer: Some(Bearer::new("tok")), ..Default::default() }, false, false, true, 0)]
    // desc: identity without a principal (auth disabled).
    #[case::corner_identity_only(RequestScope { identity: Some(SessionKey::parse("acme", "s1").unwrap()), ..Default::default() }, true, false, false, 0)]
    // desc: only a hop count (a forwarded call with auth disabled).
    #[case::corner_hops_only(RequestScope { hops: 3, ..Default::default() }, false, false, false, 3)]
    // desc: nothing installed.
    #[case::negative_empty(RequestScope::default(), false, false, false, 0)]
    #[tokio::test]
    async fn scope_request_installs_what_is_set(
        #[case] scope: RequestScope,
        #[case] identity: bool,
        #[case] principal: bool,
        #[case] bearer: bool,
        #[case] hops: u8,
    ) {
        let seen = scope_request(scope, async { RequestScope::current() }).await;
        assert_eq!(seen.identity.is_some(), identity);
        assert_eq!(seen.principal.is_some(), principal);
        assert_eq!(seen.bearer.is_some(), bearer);
        assert_eq!(seen.hops, hops);
    }

    #[rstest]
    // desc: a caller's token is forwarded on their behalf.
    #[case::positive_forwards_the_callers_token(Some("user"), Some("svc"), Some("user"))]
    // desc: no caller (a scheduler job) ⇒ the service's own token.
    #[case::positive_no_caller_uses_the_service_token(None, Some("svc"), Some("svc"))]
    // desc: a caller's token with no service token is still forwarded.
    #[case::corner_caller_without_service_source(Some("user"), None, Some("user"))]
    // desc: neither ⇒ nothing (unauthenticated, as before S9).
    #[case::negative_neither(None, None, None)]
    fn select_bearer_cases(
        #[case] forwarded: Option<&str>,
        #[case] service: Option<&str>,
        #[case] want: Option<&str>,
    ) {
        let got = select_bearer(forwarded.map(Bearer::new), service.map(Bearer::new));
        assert_eq!(got.as_ref().map(Bearer::expose), want);
    }

    #[test]
    fn boundary_hops_outside_a_request_is_zero() {
        assert_eq!(current_hops(), 0);
    }

    #[test]
    fn positive_static_bearer_source_yields_its_token() {
        let src = StaticBearer(Bearer::new("svc"));
        assert_eq!(src.bearer().as_ref().map(Bearer::expose), Some("svc"));
    }

    #[tokio::test]
    async fn positive_current_scope_survives_spawn() {
        let seen = scope_request(full(), async {
            let carried = RequestScope::current();
            tokio::spawn(scope_request(carried, async { RequestScope::current() }))
                .await
                .unwrap()
        })
        .await;
        assert_eq!(seen.bearer, Some(Bearer::new("tok")));
        assert_eq!(seen.hops, 2);
        assert_eq!(seen.principal, Some(principal()));
        assert_eq!(
            seen.identity.map(|k| k.session.as_str().to_string()),
            Some("s1".into())
        );
    }

    #[tokio::test]
    async fn negative_bare_spawn_inherits_nothing() {
        let seen = scope_request(full(), async {
            tokio::spawn(async { RequestScope::current() })
                .await
                .unwrap()
        })
        .await;
        assert!(seen.bearer.is_none() && seen.principal.is_none() && seen.identity.is_none());
        assert_eq!(seen.hops, 0);
    }

    #[test]
    fn adversarial_debug_never_prints_the_token() {
        let b = Bearer::new("eyJhbGciOiJFUzI1NiJ9.secret.sig");
        let shown = format!("{b:?} {:?}", full());
        assert!(
            !shown.contains("secret") && !shown.contains("tok\""),
            "{shown}"
        );
        assert_eq!(b.expose(), "eyJhbGciOiJFUzI1NiJ9.secret.sig");
    }
}
