//! GitHub App install onboarding (forge-auth F5c2): the **only** path that authorizes
//! `ghapp:` minting for a tenant-onboarded installation.
//!
//! The mint gate (`secrets::ghapp_installation_allowed`) reads the operator bootstrap
//! allowlist OR the ownership-verified snapshot (`secrets::GHAPP_STORE_BINDINGS`).
//! [`InstallOnboarding`] is that snapshot's sole live writer, and it writes only after:
//!
//! 1. the browser-relayed `state` is one **this** verified `(tenant, subject)` began
//!    (single-use, TTL-bounded — the CSRF tie, and a stop on cross-tenant replay);
//! 2. `prove_admin` shows the user behind the install-callback `code` **administers** the
//!    installation's account, as GitHub's App-JWT record names it (forge-auth F5c1) — the
//!    callback's `installation_id`/`setup_action` are never evidence on their own;
//! 3. no **other** tenant already holds a verified binding for the installation.
//!
//! It then persists the verified row (`agent_forge::INSTALLS_VERIFIED`, a collection no
//! CRUD RPC reaches), binds the snapshot, and upserts the display row the portal lists.
//!
//! The CRUD install store stays tenant-self-asserted and never feeds the gate: the F4b
//! `RefreshingInstalls` decorator (bind-on-`put`) was removed here, since once the gate reads
//! the snapshot, binding a raw `put` would be a cross-tenant minting IDOR.
//!
//! **Limits (documented):** a binding made on one replica reaches another only at its next
//! restart (a miss there fails closed); the cross-tenant uniqueness scan is not atomic across
//! replicas; uninstall/revoke arrives with F6 webhooks (meanwhile GitHub refuses the mint).

use std::sync::Arc;

use agent_config_store::Backend;
use agent_core::{
    safe_segment, valid_installation_id, BeginInstall, GithubInstall, GithubInstallOnboarding,
    GithubInstallRegistry, OnboardRefusal, Result,
};
use agent_forge::ghapp::{AccountKind, Clock, InstallationTokenMinter};
use agent_forge::ghapp_oauth::{UserInstallationAccess, MAX_CODE_BYTES};
use agent_forge::ghapp_onboard::{install_url, PendingStates};
use agent_forge::{StoreInstalls, VerifiedInstall, VerifiedInstalls, MAX_SUBJECT_LEN};
use async_trait::async_trait;

/// The verified caller a `state` was minted for.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Caller {
    tenant: String,
    subject: String,
}

/// Does the caller presenting a `state` match the one that began it? Pure, so the
/// cross-tenant replay rule under test is the rule that runs: both the tenant and the
/// subject must match exactly.
fn match_binding(began: &Caller, tenant: &str, subject: &str) -> bool {
    began.tenant == tenant && began.subject == subject
}

/// A proven ownership, as the onboarding needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proven {
    pub installation_id: String,
    pub account_login: String,
    /// `"User"` or `"Organization"`.
    pub account_kind: &'static str,
}

/// The admin-ownership proof, behind a seam so onboarding is tested without a forge.
/// `Ok(Some)` = proven; `Ok(None)` = answered, not an admin; `Err` = undecidable.
#[async_trait]
pub trait ProveAdmin: Send + Sync {
    async fn prove(&self, code: &str, installation_id: &str) -> Result<Option<Proven>>;
}

/// The production proof: F5c1's `UserInstallationAccess::prove_admin` with the App minter.
pub struct GithubProver {
    access: UserInstallationAccess,
    minter: Arc<InstallationTokenMinter>,
}

impl GithubProver {
    pub fn new(access: UserInstallationAccess, minter: Arc<InstallationTokenMinter>) -> Self {
        Self { access, minter }
    }
}

#[async_trait]
impl ProveAdmin for GithubProver {
    async fn prove(&self, code: &str, installation_id: &str) -> Result<Option<Proven>> {
        let proof = self
            .access
            .prove_admin(&self.minter, code, installation_id)
            .await?;
        Ok(proof.and_then(|p| {
            let account_kind = match p.account_kind() {
                AccountKind::User => "User",
                AccountKind::Organization => "Organization",
                // `prove_admin` never proves one; refuse defensively rather than store it.
                AccountKind::Other => return None,
            };
            Some(Proven {
                installation_id: p.installation_id().to_string(),
                account_login: p.account_login().to_string(),
                account_kind,
            })
        }))
    }
}

/// Where a verified binding goes once persisted. Production forwards to the process gate
/// snapshot; tests record instead of touching the global.
type BindFn = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// The [`GithubInstallOnboarding`] backend (see the module docs).
pub struct InstallOnboarding {
    pending: PendingStates<Caller>,
    prover: Arc<dyn ProveAdmin>,
    backend: Arc<dyn Backend>,
    /// The GitHub web host (`https://github.com`, or a GHES host) the install URL is on.
    web_base: String,
    slug: String,
    clock: Arc<dyn Clock>,
    bind: BindFn,
    /// The served CRUD registry the display row goes through, so it lands where `List`
    /// reads (the shared `local` view under single-operator; the caller's tenant under
    /// `per_tenant`, routed by the handler's scoped identity). `None` ⇒ write the
    /// caller's tenant directly.
    display: Option<Arc<dyn GithubInstallRegistry>>,
}

impl InstallOnboarding {
    /// Upsert the display row through `registry` (the one the CRUD service serves).
    pub fn with_display(mut self, registry: Arc<dyn GithubInstallRegistry>) -> Self {
        self.display = Some(registry);
        self
    }

    /// An onboarding backend over the install store's `backend`. `slug` must be a GitHub
    /// App slug and `web_base` an http(s) URL — checked here, so a misconfiguration is a
    /// startup error rather than a per-request one.
    pub fn new(
        prover: Arc<dyn ProveAdmin>,
        backend: Arc<dyn Backend>,
        web_base: String,
        slug: String,
        clock: Arc<dyn Clock>,
    ) -> Result<Self> {
        Self::with_bind(
            prover,
            backend,
            web_base,
            slug,
            clock,
            Arc::new(crate::secrets::ghapp_store_bind),
        )
    }

    fn with_bind(
        prover: Arc<dyn ProveAdmin>,
        backend: Arc<dyn Backend>,
        web_base: String,
        slug: String,
        clock: Arc<dyn Clock>,
        bind: BindFn,
    ) -> Result<Self> {
        if install_url(&web_base, &slug, "probe").is_none() {
            return Err(agent_core::Error::Config(
                "[github_app] slug must be a GitHub App slug (and the web host an http(s) URL)"
                    .into(),
            ));
        }
        Ok(Self {
            pending: PendingStates::new(clock.clone()),
            prover,
            backend,
            web_base,
            slug,
            clock,
            bind,
            display: None,
        })
    }

    /// The display row: the existing one's repos kept (repo-list sync is F6's
    /// `installation_repositories` webhook), stamped now.
    async fn upsert_display(&self, tenant: &str, id: &str) -> Result<GithubInstall> {
        let store: Arc<dyn GithubInstallRegistry> = match &self.display {
            Some(r) => r.clone(),
            None => Arc::new(StoreInstalls::with_tenant(self.backend.clone(), tenant)?),
        };
        let selected_repos = match store.get(id).await {
            Ok(existing) => existing.selected_repos,
            Err(_) => Vec::new(),
        };
        store
            .put(GithubInstall {
                installation_id: id.to_string(),
                selected_repos,
                installed_at: self.clock.now_secs(),
            })
            .await
    }
}

/// A verified principal's subject we can persist (see `VerifiedInstall::validate`).
fn valid_subject(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_SUBJECT_LEN && !s.chars().any(char::is_control)
}

#[async_trait]
impl GithubInstallOnboarding for InstallOnboarding {
    async fn begin(
        &self,
        tenant: &str,
        subject: &str,
    ) -> std::result::Result<BeginInstall, OnboardRefusal> {
        if !safe_segment(tenant) || !valid_subject(subject) {
            return Err(OnboardRefusal::Malformed);
        }
        let (state, expires_at) = self
            .pending
            .insert(Caller {
                tenant: tenant.to_string(),
                subject: subject.to_string(),
            })
            .map_err(|_| OnboardRefusal::Busy)?;
        let install_url =
            install_url(&self.web_base, &self.slug, &state).ok_or(OnboardRefusal::NotConfigured)?;
        Ok(BeginInstall {
            install_url,
            state,
            expires_at,
        })
    }

    async fn complete(
        &self,
        tenant: &str,
        subject: &str,
        state: &str,
        code: &str,
        installation_id: &str,
    ) -> std::result::Result<GithubInstall, OnboardRefusal> {
        // Spend the state first, whatever follows: a state is good for one attempt.
        let began = self
            .pending
            .take(state)
            .ok_or(OnboardRefusal::UnknownState)?;
        if !match_binding(&began, tenant, subject) {
            tracing::warn!("install onboarding: state presented by a different caller");
            return Err(OnboardRefusal::UnknownState);
        }
        if !valid_installation_id(installation_id) || code.is_empty() || code.len() > MAX_CODE_BYTES
        {
            return Err(OnboardRefusal::Malformed);
        }
        let proven = match self.prover.prove(code, installation_id).await {
            Ok(Some(p)) if p.installation_id == installation_id => p,
            Ok(_) => return Err(OnboardRefusal::NotProven),
            Err(e) => {
                tracing::warn!(error = %e, "install onboarding: ownership proof undecidable");
                return Err(OnboardRefusal::Unavailable);
            }
        };
        // One tenant per installation: a second tenant can never claim a bound install,
        // even as a proven admin (e.g. one person holding accounts in two tenants).
        match agent_forge::verified_owner(&self.backend, installation_id).await {
            Ok(Some(owner)) if owner != tenant => return Err(OnboardRefusal::Claimed),
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(error = %e, "install onboarding: verified-store scan failed");
                return Err(OnboardRefusal::Unavailable);
            }
        }
        let row = VerifiedInstall {
            installation_id: installation_id.to_string(),
            account_login: proven.account_login,
            account_kind: proven.account_kind.to_string(),
            subject: subject.to_string(),
            verified_at: self.clock.now_secs(),
        };
        let persisted = match VerifiedInstalls::with_tenant(self.backend.clone(), tenant) {
            Ok(store) => store.put(&row).await,
            Err(e) => Err(e),
        };
        if let Err(e) = persisted {
            tracing::warn!(error = %e, "install onboarding: verified row not persisted");
            return Err(OnboardRefusal::Unavailable);
        }
        // Bind only after the verified row persisted, so the snapshot never holds an id
        // the boot seed would not rebuild.
        (self.bind)(tenant, installation_id);
        tracing::info!(tenant, installation_id, "github app installation onboarded");
        match self.upsert_display(tenant, installation_id).await {
            Ok(install) => Ok(install),
            Err(e) => {
                // The authoritative binding is in place; only the listing row is missing.
                tracing::warn!(error = %e, "install onboarding: display row not written");
                Ok(GithubInstall {
                    installation_id: installation_id.to_string(),
                    selected_repos: Vec::new(),
                    installed_at: row.verified_at,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_config_store::MemoryBackend;
    use rstest::rstest;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now_secs(&self) -> u64 {
            1_700_000_000
        }
    }

    /// A scripted proof: `Some(kind)` proves with that account kind, `None` answers
    /// "not admin", and `err` makes the proof undecidable. Counts calls.
    struct FakeProver {
        outcome: Option<&'static str>,
        err: bool,
        calls: AtomicUsize,
    }
    impl FakeProver {
        fn new(outcome: Option<&'static str>, err: bool) -> Arc<Self> {
            Arc::new(Self {
                outcome,
                err,
                calls: AtomicUsize::new(0),
            })
        }
    }
    #[async_trait]
    impl ProveAdmin for FakeProver {
        async fn prove(&self, _code: &str, id: &str) -> Result<Option<Proven>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.err {
                return Err(agent_core::Error::Repo("forge unreachable".into()));
            }
            Ok(self.outcome.map(|kind| Proven {
                installation_id: id.to_string(),
                account_login: "acme".into(),
                account_kind: kind,
            }))
        }
    }

    type Binds = Arc<Mutex<Vec<(String, String)>>>;

    struct Rig {
        onboard: InstallOnboarding,
        prover: Arc<FakeProver>,
        backend: Arc<dyn Backend>,
        binds: Binds,
    }

    fn rig_on(backend: Arc<dyn Backend>, prover: Arc<FakeProver>) -> Rig {
        let binds: Binds = Arc::default();
        let b = binds.clone();
        let onboard = InstallOnboarding::with_bind(
            prover.clone(),
            backend.clone(),
            "https://github.com".into(),
            "seddon-agent".into(),
            Arc::new(FixedClock),
            Arc::new(move |t, id| b.lock().unwrap().push((t.to_string(), id.to_string()))),
        )
        .expect("valid config");
        Rig {
            onboard,
            prover,
            backend,
            binds,
        }
    }

    fn rig(prover: Arc<FakeProver>) -> Rig {
        rig_on(Arc::new(MemoryBackend::new()), prover)
    }

    async fn begin(r: &Rig, tenant: &str, subject: &str) -> String {
        r.onboard.begin(tenant, subject).await.expect("begin").state
    }

    async fn verified_ids(r: &Rig, tenant: &str) -> Vec<String> {
        VerifiedInstalls::with_tenant(r.backend.clone(), tenant)
            .unwrap()
            .list()
            .await
            .unwrap()
            .into_iter()
            .map(|v| v.installation_id)
            .collect()
    }

    // The pure caller-match rule: the state's (tenant, subject) must match exactly.
    #[rstest]
    #[case::positive_same_caller("acme", "alice", true)]
    #[case::negative_other_subject_same_tenant("acme", "bob", false)]
    #[case::negative_other_tenant_same_subject("globex", "alice", false)]
    #[case::corner_empty_caller("", "", false)]
    #[case::adversarial_cross_tenant_replay("globex", "mallory", false)]
    #[case::adversarial_case_folded_tenant("ACME", "alice", false)]
    fn match_binding_cases(#[case] tenant: &str, #[case] subject: &str, #[case] ok: bool) {
        let began = Caller {
            tenant: "acme".into(),
            subject: "alice".into(),
        };
        assert_eq!(match_binding(&began, tenant, subject), ok);
    }

    #[tokio::test]
    async fn positive_begin_returns_install_url_with_state() {
        let r = rig(FakeProver::new(Some("Organization"), false));
        let b = r.onboard.begin("acme", "alice").await.unwrap();
        assert_eq!(
            b.install_url,
            format!(
                "https://github.com/apps/seddon-agent/installations/new?state={}",
                b.state
            )
        );
        assert_eq!(
            b.expires_at,
            1_700_000_000 + agent_forge::ghapp_onboard::STATE_TTL_SECS
        );
    }

    // Both proven account kinds onboard: verified row written, gate bound, display row
    // upserted for the caller's tenant.
    #[rstest]
    #[case::positive_org_admin("Organization")]
    #[case::positive_user_owner("User")]
    #[tokio::test]
    async fn positive_complete_binds_and_records(#[case] kind: &'static str) {
        let r = rig(FakeProver::new(Some(kind), false));
        let state = begin(&r, "acme", "alice").await;
        let install = r
            .onboard
            .complete("acme", "alice", &state, "code", "42")
            .await
            .expect("onboarded");
        assert_eq!(install.installation_id, "42");
        assert_eq!(verified_ids(&r, "acme").await, vec!["42".to_string()]);
        assert_eq!(
            r.binds.lock().unwrap().as_slice(),
            &[("acme".to_string(), "42".to_string())]
        );
        let display = StoreInstalls::with_tenant(r.backend.clone(), "acme").unwrap();
        assert_eq!(display.get("42").await.unwrap().installed_at, 1_700_000_000);
    }

    // corner: with a served registry attached (single-operator's shared `local` view), the
    // display row lands there — where `List` reads — not under the principal's tenant key.
    #[tokio::test]
    async fn corner_display_row_goes_through_served_registry() {
        let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        let shared: Arc<dyn GithubInstallRegistry> = Arc::new(StoreInstalls::new(backend.clone()));
        let mut r = rig_on(
            backend.clone(),
            FakeProver::new(Some("Organization"), false),
        );
        r.onboard = r.onboard.with_display(shared.clone());
        let state = begin(&r, "acme", "alice").await;
        r.onboard
            .complete("acme", "alice", &state, "code", "42")
            .await
            .unwrap();
        assert!(shared.get("42").await.is_ok(), "listed in the served view");
        let per_tenant = StoreInstalls::with_tenant(backend, "acme").unwrap();
        assert!(per_tenant.get("42").await.is_err());
        // The authoritative binding is still the caller's tenant.
        assert_eq!(verified_ids(&r, "acme").await, vec!["42".to_string()]);
    }

    // The display upsert keeps repos an earlier CRUD row recorded.
    #[tokio::test]
    async fn corner_complete_keeps_existing_display_repos() {
        let r = rig(FakeProver::new(Some("Organization"), false));
        let display = StoreInstalls::with_tenant(r.backend.clone(), "acme").unwrap();
        display
            .put(GithubInstall {
                installation_id: "42".into(),
                selected_repos: vec!["acme__app".into()],
                installed_at: 1,
            })
            .await
            .unwrap();
        let state = begin(&r, "acme", "alice").await;
        let got = r
            .onboard
            .complete("acme", "alice", &state, "code", "42")
            .await
            .unwrap();
        assert_eq!(got.selected_repos, vec!["acme__app".to_string()]);
    }

    // Every refusal leaves nothing written and nothing bound. `proved` is whether the
    // forge proof was consulted at all (state/shape refusals stop before it).
    #[rstest]
    #[case::negative_not_admin(None, false, OnboardRefusal::NotProven, true)]
    #[case::negative_proof_undecidable(
        Some("Organization"),
        true,
        OnboardRefusal::Unavailable,
        true
    )]
    #[tokio::test]
    async fn negative_proof_refusals_write_nothing(
        #[case] outcome: Option<&'static str>,
        #[case] err: bool,
        #[case] want: OnboardRefusal,
        #[case] proved: bool,
    ) {
        let r = rig(FakeProver::new(outcome, err));
        let state = begin(&r, "acme", "alice").await;
        let got = r
            .onboard
            .complete("acme", "alice", &state, "code", "42")
            .await;
        assert_eq!(got, Err(want));
        assert_eq!(r.prover.calls.load(Ordering::SeqCst) > 0, proved);
        assert!(verified_ids(&r, "acme").await.is_empty());
        assert!(r.binds.lock().unwrap().is_empty());
    }

    // State failures are opaque and never reach the forge.
    #[rstest]
    #[case::negative_unknown_state("acme", "alice", Some("0".repeat(64)))]
    #[case::boundary_empty_state("acme", "alice", Some(String::new()))]
    #[case::adversarial_other_tenant_replays_state("globex", "alice", None)]
    #[case::adversarial_other_subject_replays_state("acme", "mallory", None)]
    #[tokio::test]
    async fn adversarial_state_refusals_never_prove(
        #[case] tenant: &str,
        #[case] subject: &str,
        #[case] state_override: Option<String>,
    ) {
        let r = rig(FakeProver::new(Some("Organization"), false));
        let real = begin(&r, "acme", "alice").await;
        let state = state_override.unwrap_or_else(|| real.clone());
        let got = r
            .onboard
            .complete(tenant, subject, &state, "code", "42")
            .await;
        assert_eq!(got, Err(OnboardRefusal::UnknownState));
        assert_eq!(r.prover.calls.load(Ordering::SeqCst), 0, "no forge call");
        assert!(r.binds.lock().unwrap().is_empty());
    }

    // A replayed state (even by the right caller) is spent: the second attempt is refused.
    #[tokio::test]
    async fn negative_state_is_spent_by_first_use() {
        let r = rig(FakeProver::new(None, false));
        let state = begin(&r, "acme", "alice").await;
        let _ = r
            .onboard
            .complete("acme", "alice", &state, "code", "42")
            .await;
        let again = r
            .onboard
            .complete("acme", "alice", &state, "code", "42")
            .await;
        assert_eq!(again, Err(OnboardRefusal::UnknownState));
    }

    // A wrong caller burns the state too, so the rightful caller must begin again (a
    // hostile replay cannot be retried into a win, and never lands a binding).
    #[tokio::test]
    async fn adversarial_mismatched_caller_spends_state() {
        let r = rig(FakeProver::new(Some("Organization"), false));
        let state = begin(&r, "acme", "alice").await;
        let _ = r
            .onboard
            .complete("globex", "alice", &state, "code", "42")
            .await;
        let rightful = r
            .onboard
            .complete("acme", "alice", &state, "code", "42")
            .await;
        assert_eq!(rightful, Err(OnboardRefusal::UnknownState));
        assert!(r.binds.lock().unwrap().is_empty());
    }

    // Hostile callback values are refused before the forge is asked.
    #[rstest]
    #[case::adversarial_traversal_id("code", "../42")]
    #[case::adversarial_over_len_id("code", "999999999999999999999")]
    #[case::boundary_empty_code("", "42")]
    #[case::adversarial_huge_code("c".repeat(MAX_CODE_BYTES + 1).leak(), "42")]
    #[tokio::test]
    async fn adversarial_malformed_callback_rejected_before_proof(
        #[case] code: &'static str,
        #[case] id: &str,
    ) {
        let r = rig(FakeProver::new(Some("Organization"), false));
        let state = begin(&r, "acme", "alice").await;
        let got = r.onboard.complete("acme", "alice", &state, code, id).await;
        assert_eq!(got, Err(OnboardRefusal::Malformed));
        assert_eq!(r.prover.calls.load(Ordering::SeqCst), 0);
    }

    // One tenant per installation: once acme holds 42, globex — even as a proven admin —
    // is refused, and acme's binding is untouched.
    #[tokio::test]
    async fn adversarial_second_tenant_cannot_claim_bound_install() {
        let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        let acme = rig_on(
            backend.clone(),
            FakeProver::new(Some("Organization"), false),
        );
        let s = begin(&acme, "acme", "alice").await;
        acme.onboard
            .complete("acme", "alice", &s, "code", "42")
            .await
            .unwrap();
        let globex = rig_on(backend, FakeProver::new(Some("Organization"), false));
        let s = begin(&globex, "globex", "bob").await;
        let got = globex
            .onboard
            .complete("globex", "bob", &s, "code", "42")
            .await;
        assert_eq!(got, Err(OnboardRefusal::Claimed));
        assert!(globex.binds.lock().unwrap().is_empty());
        assert!(verified_ids(&globex, "globex").await.is_empty());
        assert_eq!(verified_ids(&acme, "acme").await, vec!["42".to_string()]);
    }

    // The same tenant re-onboarding its own installation is idempotent, not Claimed.
    #[tokio::test]
    async fn corner_same_tenant_reonboard_is_idempotent() {
        let r = rig(FakeProver::new(Some("Organization"), false));
        for _ in 0..2 {
            let s = begin(&r, "acme", "alice").await;
            r.onboard
                .complete("acme", "alice", &s, "code", "42")
                .await
                .unwrap();
        }
        assert_eq!(verified_ids(&r, "acme").await, vec!["42".to_string()]);
    }

    // A CRUD row naming an installation (tenant-self-asserted) grants nothing: it is
    // neither a binding nor a claim that blocks the rightful owner.
    #[tokio::test]
    async fn adversarial_crud_row_is_not_a_claim() {
        let r = rig(FakeProver::new(Some("Organization"), false));
        StoreInstalls::with_tenant(r.backend.clone(), "mallory")
            .unwrap()
            .put(GithubInstall {
                installation_id: "42".into(),
                selected_repos: vec![],
                installed_at: 1,
            })
            .await
            .unwrap();
        let s = begin(&r, "acme", "alice").await;
        assert!(r
            .onboard
            .complete("acme", "alice", &s, "code", "42")
            .await
            .is_ok());
        assert_eq!(
            r.binds.lock().unwrap().as_slice(),
            &[("acme".to_string(), "42".to_string())]
        );
    }

    #[rstest]
    #[case::adversarial_traversal_tenant("../etc", "alice")]
    #[case::negative_empty_subject("acme", "")]
    #[case::adversarial_crlf_subject("acme", "a\r\nb")]
    #[case::boundary_subject_over_cap("acme", &"s".repeat(MAX_SUBJECT_LEN + 1))]
    #[tokio::test]
    async fn adversarial_begin_rejects_unpersistable_caller(
        #[case] tenant: &str,
        #[case] subject: &str,
    ) {
        let r = rig(FakeProver::new(Some("Organization"), false));
        assert_eq!(
            r.onboard.begin(tenant, subject).await,
            Err(OnboardRefusal::Malformed)
        );
        assert!(r.onboard.pending.is_empty());
    }

    #[rstest]
    #[case::negative_empty_slug("https://github.com", "")]
    #[case::adversarial_traversal_slug("https://github.com", "../x")]
    #[case::negative_bad_web_base("not a url", "seddon-agent")]
    fn negative_bad_config_is_a_startup_error(#[case] web: &str, #[case] slug: &str) {
        let got = InstallOnboarding::new(
            FakeProver::new(None, false),
            Arc::new(MemoryBackend::new()),
            web.into(),
            slug.into(),
            Arc::new(FixedClock),
        );
        assert!(got.is_err());
    }
}
