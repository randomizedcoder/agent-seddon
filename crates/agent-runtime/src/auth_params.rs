//! `[auth]` config as the gRPC auth layer's parameters: one mapping shared by the
//! serve path (`agent-cli`) and `agent doctor`, so both see the same issuers.

use crate::config::{AuthCfg, AuthIssuerCfg};
use agent_grpc::server::{AuthParams, ClientSecret, IssuerParams};

/// One `[[auth.issuers]]` entry as the verifier's params (field for field).
pub fn issuer_params(c: &AuthIssuerCfg) -> IssuerParams {
    IssuerParams {
        name: c.name.clone(),
        profile: c.profile.clone(),
        issuer: c.issuer.clone(),
        audience: c.audience.clone(),
        jwks_url: c.jwks_url.clone(),
        tenant_claim: c.tenant_claim.clone(),
        subject_claim: c.subject_claim.clone(),
        roles_claim: c.roles_claim.clone(),
        trust_roles_claim: c.trust_roles_claim,
        require_email_verified: c.require_email_verified,
        allowed_domains: c.allowed_domains.clone(),
        allowed_tenants: c.allowed_tenants.clone(),
        default_tenant: c.default_tenant.clone(),
        client_secret: None,
    }
}

/// [`issuer_params`] for the serve path: with browser sign-in on (`[auth]
/// redirect_uris` set, `browser_sign_in`), the issuer's `client_secret` reference
/// is resolved here, since
/// the agent redeems the code. A reference that does not resolve refuses to start
/// rather than serving a sign-in that cannot finish. Off, nothing is read.
pub fn serving_issuer_params(
    c: &AuthIssuerCfg,
    browser_sign_in: bool,
) -> Result<IssuerParams, String> {
    let mut p = issuer_params(c);
    if browser_sign_in && !c.client_secret.trim().is_empty() {
        let secret =
            crate::secrets::resolve(crate::secrets::SecretScope::Operator, &c.client_secret)
                .map_err(|e| {
                    format!("`[[auth.issuers]]` `{}` client_secret: {e}", c.name.trim())
                })?;
        p.client_secret = Some(ClientSecret::new(secret.expose()));
    }
    Ok(p)
}

/// Every login issuer `[auth]` configures: the legacy single-issuer keys (as
/// `default`) followed by `[[auth.issuers]]`, exactly as the verifier builds them.
pub fn login_issuers(a: &AuthCfg) -> Vec<IssuerParams> {
    AuthParams {
        mode: a.mode.clone(),
        issuer: a.issuer.clone(),
        audience: a.audience.clone(),
        jwks_url: a.jwks_url.clone(),
        tenant_claim: a.tenant_claim.clone(),
        roles_claim: a.roles_claim.clone(),
        leeway_secs: a.leeway_secs,
        issuers: a.issuers.iter().map(issuer_params).collect(),
        ..AuthParams::default()
    }
    .issuer_list()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn entry(secret_ref: &str) -> AuthIssuerCfg {
        AuthIssuerCfg {
            name: "google".into(),
            profile: "google".into(),
            audience: "web".into(),
            client_secret: secret_ref.into(),
            ..AuthIssuerCfg::default()
        }
    }

    /// With browser sign-in on, the reference is read; off, it is not (a secret
    /// only `agent login` has must not stop the server). A missing file refuses
    /// to start, naming the issuer but never the value.
    #[rstest]
    #[case::positive_on_resolves(true, true, Ok(Some("s3cret")))]
    #[case::corner_off_reads_nothing(false, false, Ok(None))]
    #[case::negative_on_missing_file(true, false, Err("`google` client_secret"))]
    fn serving_secret(
        #[case] browser_sign_in: bool,
        #[case] file_exists: bool,
        #[case] want: Result<Option<&str>, &str>,
    ) {
        let dir = agent_testkit::tempdir();
        let path = dir.join("client-secret");
        if file_exists {
            std::fs::write(&path, "s3cret\n").expect("write secret");
        }
        let got =
            serving_issuer_params(&entry(&format!("file:{}", path.display())), browser_sign_in);
        match (got, want) {
            (Ok(p), Ok(secret)) => {
                assert_eq!(p.client_secret.as_ref().map(ClientSecret::expose), secret);
            }
            (Err(e), Err(fragment)) => {
                assert!(e.contains(fragment), "{e}");
                assert!(!e.contains("s3cret"));
            }
            (got, want) => panic!("got {got:?}, want {want:?}"),
        }
    }

    #[test]
    fn corner_no_reference_is_a_public_client() {
        let p = serving_issuer_params(&entry(""), true).expect("no secret needed");
        assert!(p.client_secret.is_none());
    }
}
