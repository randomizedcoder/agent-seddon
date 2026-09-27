//! Service identity from mutual TLS (security-hardening S10,
//! docs/design/security-hardening/04-service-integration.md#mtls-between-services).
//!
//! `[auth.mtls] bindings` maps a client certificate's URI SAN (its SPIFFE id) to a
//! service principal: a name (the agent subject becomes `svc:<name>`), the tenant it
//! acts in, and its roles. A verified peer whose SAN is bound is a **known service**:
//!
//! - it may trade its certificate for a service token at `AuthService.Exchange`
//!   (`use_client_cert`), and that token is bound to the certificate (`cnf`);
//! - it may relay a service token another known service was issued (the forwarding
//!   of S9), because it too holds a certificate from the cluster CA and a binding.
//!
//! A peer whose SAN is not bound is an ordinary client: a user token over its
//! connection works as before, with no service identity attached.

use std::collections::HashMap;

use super::peer::PeerCert;
use super::MtlsBindingParams;

/// Most bindings one process accepts.
pub const MAX_MTLS_BINDINGS: usize = 256;
/// Most roles one binding grants.
pub const MAX_MTLS_ROLES: usize = 32;
/// The only SAN scheme a binding may name.
pub const SAN_SCHEME: &str = "spiffe://";

/// A validated binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceBinding {
    pub san: String,
    pub service: String,
    pub tenant: String,
    pub roles: Vec<String>,
}

impl ServiceBinding {
    /// The agent subject a service token carries.
    pub fn subject(&self) -> String {
        format!("svc:{}", self.service)
    }
}

/// Whether `san` is a URI SAN a binding may name: `spiffe://`, printable ASCII, no
/// whitespace, bounded.
pub fn valid_san(san: &str) -> bool {
    san.len() <= super::peer::MAX_URI_BYTES
        && san.len() > SAN_SCHEME.len()
        && san.starts_with(SAN_SCHEME)
        && san.bytes().all(|b| b.is_ascii_graphic())
}

/// Every configured binding, keyed by SAN.
#[derive(Clone, Debug, Default)]
pub struct MtlsBindings {
    by_san: HashMap<String, ServiceBinding>,
}

impl MtlsBindings {
    /// Validate `[auth.mtls] bindings`. Refused: a SAN that is not `spiffe://…`, a
    /// service name or tenant that is not a safe segment, a missing or unsafe role,
    /// the same SAN or service name twice, and more than the caps.
    pub fn parse(entries: &[MtlsBindingParams]) -> Result<Self, String> {
        if entries.len() > MAX_MTLS_BINDINGS {
            return Err(format!(
                "`[auth.mtls] bindings` has more than {MAX_MTLS_BINDINGS} entries"
            ));
        }
        let mut by_san = HashMap::new();
        let mut services = std::collections::HashSet::new();
        for e in entries {
            let san = e.san.trim();
            let at = |what: &str| format!("`[auth.mtls] bindings` entry `{san}`: {what}");
            if !valid_san(san) {
                return Err(at("`san` must be a `spiffe://` URI SAN"));
            }
            let service = e.service.trim();
            if !agent_core::safe_segment(service) {
                return Err(at("`service` must be a plain name"));
            }
            let tenant = e.tenant.trim();
            if !agent_core::safe_segment(tenant) {
                return Err(at("`tenant` must be a plain name"));
            }
            if e.roles.is_empty() || e.roles.len() > MAX_MTLS_ROLES {
                return Err(at(&format!("needs 1..={MAX_MTLS_ROLES} `roles`")));
            }
            let roles: Vec<String> = e.roles.iter().map(|r| r.trim().to_string()).collect();
            if !roles.iter().all(|r| agent_core::safe_segment(r)) {
                return Err(at("a role is not a plain name"));
            }
            if !services.insert(service.to_string()) {
                return Err(at(&format!("service `{service}` is bound twice")));
            }
            let binding = ServiceBinding {
                san: san.to_string(),
                service: service.to_string(),
                tenant: tenant.to_string(),
                roles,
            };
            if by_san.insert(san.to_string(), binding).is_some() {
                return Err(at("the SAN is bound twice"));
            }
        }
        Ok(Self { by_san })
    }

    pub fn is_empty(&self) -> bool {
        self.by_san.is_empty()
    }

    /// The binding for a verified peer. A certificate with no bound SAN is not a
    /// known service; one whose SANs match **two** bindings is refused as
    /// ambiguous rather than picking one.
    pub fn service_of(&self, peer: &PeerCert) -> Option<&ServiceBinding> {
        let mut hits = peer.uris.iter().filter_map(|u| self.by_san.get(u));
        let first = hits.next()?;
        match hits.next() {
            None => Some(first),
            Some(_) => {
                tracing::warn!("client certificate matches two `[auth.mtls]` bindings: ignored");
                None
            }
        }
    }
}

/// Whether a token bound to a certificate (`cnf`) may be used on this connection:
/// the peer presented that certificate, or the peer is a known service relaying a
/// token downstream (S9 forwarding keeps the token and changes the connection). A
/// token with no `cnf` (a person's) needs no certificate. Without a peer
/// certificate a bound token is always refused: a stolen service token is useless
/// off the cluster's mutual-TLS connections.
pub fn cnf_allows(cnf: Option<&str>, peer: Option<&PeerCert>, bindings: &MtlsBindings) -> bool {
    let Some(cnf) = cnf else {
        return true;
    };
    match peer {
        None => false,
        Some(p) => p.thumbprint == cnf || bindings.service_of(p).is_some(),
    }
}

#[cfg(test)]
mod tests;
