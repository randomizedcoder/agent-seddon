//! GitHub App webhook events → fleet actions (forge-auth F6), the transport-free core.
//!
//! The HTTP receiver (in `agent-cli`) verifies the `X-Hub-Signature-256` HMAC over the raw
//! body **before** anything here runs, then hands the event name and body to [`parse`] and
//! the result to [`dispatch`]. Even signed, the payload is data from the network: every
//! field used is shape-checked (installation ids are decimal, repo names are a GitHub login
//! plus a path-safe name, PR numbers are positive and bounded, lists are capped) and a
//! payload that fails any check is rejected whole — never partially applied.
//!
//! What an event can do is deliberately narrow:
//! - `pull_request` (`opened`/`reopened`/`synchronize`/`ready_for_review`, not draft) →
//!   one review trigger per enabled roster row, **in the installation's owning tenant**,
//!   watching that repo. The tenant comes from the verified install claim, never from the
//!   payload; an installation nobody claimed triggers nothing.
//! - `installation` `deleted` / `suspend` → revoke that installation for its owner
//!   (narrows the `ghapp:` mint gate); `unsuspend` → restore it only if it is still
//!   verified. `created` does nothing: an install is bound only by the verified onboarding
//!   flow (F5c2), never by a webhook.
//! - `installation_repositories` → keep the owner's install record's repo list in sync.
//!   This is display data; it never touches the mint gate.

use agent_core::{
    safe_segment, valid_github_login, valid_installation_id, FleetSession, FleetTrigger, Result,
    TriggerOutcome, MAX_SELECTED_REPOS,
};
use async_trait::async_trait;
use serde_json::Value;

/// Who a webhook-driven review is attributed to (S19 `requested_by`).
pub const WEBHOOK_REQUESTER: &str = "github-webhook";

/// Longest repository name GitHub allows.
pub const MAX_REPO_NAME_LEN: usize = 100;

/// A webhook payload that is understood but refused (malformed or out of bounds). The
/// receiver answers it with a client error and logs the reason — never the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reject(pub &'static str);

/// What a verified webhook delivery asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookEvent {
    /// GitHub's hook-created handshake.
    Ping,
    /// A PR that should be (re)reviewed.
    PullRequest {
        installation: String,
        /// The roster's `owner__name` slug.
        repo: String,
        pr: u64,
    },
    InstallDeleted {
        installation: String,
    },
    InstallSuspended {
        installation: String,
    },
    InstallUnsuspended {
        installation: String,
    },
    /// Repos added to / removed from an installation (`owner__name` slugs).
    ReposChanged {
        installation: String,
        added: Vec<String>,
        removed: Vec<String>,
    },
    /// A well-formed event or action the fleet does not act on.
    Ignored,
}

/// Parse a verified delivery. `event` is the `X-GitHub-Event` header.
pub fn parse(event: &str, body: &[u8]) -> std::result::Result<WebhookEvent, Reject> {
    match event {
        "ping" => return Ok(WebhookEvent::Ping),
        "pull_request" | "installation" | "installation_repositories" => {}
        _ => return Ok(WebhookEvent::Ignored),
    }
    let v: Value = serde_json::from_slice(body).map_err(|_| Reject("body is not JSON"))?;
    let action = v
        .get("action")
        .and_then(Value::as_str)
        .ok_or(Reject("missing action"))?;
    match event {
        "pull_request" => {
            if !matches!(
                action,
                "opened" | "reopened" | "synchronize" | "ready_for_review"
            ) {
                return Ok(WebhookEvent::Ignored);
            }
            let installation = installation_id(&v)?;
            let pr = v
                .get("number")
                .and_then(Value::as_u64)
                .filter(|n| (1..=u64::from(u32::MAX)).contains(n))
                .ok_or(Reject("pull request number out of range"))?;
            let draft = match v.pointer("/pull_request/draft") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(b)) => *b,
                Some(_) => return Err(Reject("pull_request.draft is not a bool")),
            };
            let repo = v
                .pointer("/repository/full_name")
                .and_then(Value::as_str)
                .and_then(repo_slug)
                .ok_or(Reject("repository name is not a valid owner/name"))?;
            if draft {
                return Ok(WebhookEvent::Ignored);
            }
            Ok(WebhookEvent::PullRequest {
                installation,
                repo,
                pr,
            })
        }
        "installation" => {
            let installation = installation_id(&v)?;
            Ok(match action {
                "deleted" => WebhookEvent::InstallDeleted { installation },
                "suspend" => WebhookEvent::InstallSuspended { installation },
                "unsuspend" => WebhookEvent::InstallUnsuspended { installation },
                _ => WebhookEvent::Ignored,
            })
        }
        _ => {
            if !matches!(action, "added" | "removed") {
                return Ok(WebhookEvent::Ignored);
            }
            let installation = installation_id(&v)?;
            Ok(WebhookEvent::ReposChanged {
                installation,
                added: repo_list(&v, "repositories_added")?,
                removed: repo_list(&v, "repositories_removed")?,
            })
        }
    }
}

/// `installation.id` as the decimal string the gate and stores use.
fn installation_id(v: &Value) -> std::result::Result<String, Reject> {
    v.pointer("/installation/id")
        .and_then(Value::as_u64)
        .map(|n| n.to_string())
        .filter(|id| valid_installation_id(id))
        .ok_or(Reject("missing or invalid installation id"))
}

/// A GitHub `owner/name` as the roster's `owner__name` slug. The owner must be a GitHub
/// login (so it holds no `_` and the slug decodes back unambiguously) and the name a
/// path-safe segment no longer than GitHub allows.
pub fn repo_slug(full_name: &str) -> Option<String> {
    let (owner, name) = full_name.split_once('/')?;
    let ok = valid_github_login(owner)
        && name.len() <= MAX_REPO_NAME_LEN
        && safe_segment(name)
        && !name.contains('/');
    ok.then(|| format!("{owner}__{name}"))
}

/// A capped list of repo objects' `full_name`s as slugs; absent ⇒ empty. Any bad entry
/// rejects the whole event.
fn repo_list(v: &Value, field: &str) -> std::result::Result<Vec<String>, Reject> {
    let Some(list) = v.get(field) else {
        return Ok(Vec::new());
    };
    let list = list
        .as_array()
        .ok_or(Reject("repository list is not a list"))?;
    if list.len() > MAX_SELECTED_REPOS {
        return Err(Reject("repository list over the cap"));
    }
    list.iter()
        .map(|r| {
            r.get("full_name")
                .and_then(Value::as_str)
                .and_then(repo_slug)
                .ok_or(Reject("repository name is not a valid owner/name"))
        })
        .collect()
}

/// What [`dispatch`] acts through: the fleet's view of installs and the roster, and the
/// trigger intake. Implemented over the real stores in `agent-runtime`; faked in tests.
#[async_trait]
pub trait WebhookTarget: Send + Sync {
    /// The tenant owning `installation`: `Some("")` on a single-operator fleet (every
    /// installation of the app is the operator's), the verified claim's tenant on a
    /// per-tenant one, `None` when nobody holds it (the event is dropped).
    async fn owner_of(&self, installation: &str) -> Result<Option<String>>;
    /// `tenant`'s enabled roster rows that reach their forge through GitHub, read under
    /// that tenant's scope.
    async fn github_rows(&self, tenant: &str) -> Result<Vec<FleetSession>>;
    fn enqueue(&self, trigger: FleetTrigger) -> TriggerOutcome;
    /// The installation is gone: unbind it from `tenant` and forget it.
    async fn revoke(&self, tenant: &str, installation: &str) -> Result<()>;
    /// The installation is suspended: stop minting for it, keep its records.
    async fn suspend(&self, tenant: &str, installation: &str) -> Result<()>;
    /// The installation is back: restore it only if it is still verified for `tenant`.
    async fn unsuspend(&self, tenant: &str, installation: &str) -> Result<()>;
    /// Apply a repo-list change to `tenant`'s record of the installation.
    async fn sync_repos(
        &self,
        tenant: &str,
        installation: &str,
        added: &[String],
        removed: &[String],
    ) -> Result<()>;
}

/// What [`dispatch`] did, for the receiver's log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dispatched {
    Pong,
    Ignored,
    /// No tenant holds the installation.
    Unclaimed,
    /// `n` review triggers enqueued.
    Triggered(usize),
    Revoked,
    Suspended,
    Unsuspended,
    ReposSynced,
}

/// Act on a parsed event.
pub async fn dispatch(event: WebhookEvent, target: &dyn WebhookTarget) -> Result<Dispatched> {
    let installation = match &event {
        WebhookEvent::Ping => return Ok(Dispatched::Pong),
        WebhookEvent::Ignored => return Ok(Dispatched::Ignored),
        WebhookEvent::PullRequest { installation, .. }
        | WebhookEvent::InstallDeleted { installation }
        | WebhookEvent::InstallSuspended { installation }
        | WebhookEvent::InstallUnsuspended { installation }
        | WebhookEvent::ReposChanged { installation, .. } => installation.clone(),
    };
    let Some(tenant) = target.owner_of(&installation).await? else {
        return Ok(Dispatched::Unclaimed);
    };
    Ok(match event {
        WebhookEvent::PullRequest { repo, pr, .. } => {
            let mut n = 0;
            for row in target.github_rows(&tenant).await? {
                if row.enabled && row.repo == repo {
                    target.enqueue(FleetTrigger {
                        tenant: tenant.clone(),
                        session_id: row.id,
                        pr_number: pr,
                        requested_by: vec![WEBHOOK_REQUESTER.to_string()],
                    });
                    n += 1;
                }
            }
            Dispatched::Triggered(n)
        }
        WebhookEvent::InstallDeleted { .. } => {
            target.revoke(&tenant, &installation).await?;
            Dispatched::Revoked
        }
        WebhookEvent::InstallSuspended { .. } => {
            target.suspend(&tenant, &installation).await?;
            Dispatched::Suspended
        }
        WebhookEvent::InstallUnsuspended { .. } => {
            target.unsuspend(&tenant, &installation).await?;
            Dispatched::Unsuspended
        }
        WebhookEvent::ReposChanged { added, removed, .. } => {
            target
                .sync_repos(&tenant, &installation, &added, &removed)
                .await?;
            Dispatched::ReposSynced
        }
        WebhookEvent::Ping | WebhookEvent::Ignored => unreachable!("returned above"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn pr_body(action: &str, number: Value, draft: Value, full_name: &str, inst: Value) -> Vec<u8> {
        serde_json::json!({
            "action": action,
            "number": number,
            "pull_request": { "draft": draft, "title": "ignored" },
            "repository": { "full_name": full_name, "private": true },
            "installation": { "id": inst },
            "sender": { "login": "someone" },
        })
        .to_string()
        .into_bytes()
    }

    fn pr(action: &str) -> Vec<u8> {
        pr_body(action, 7.into(), false.into(), "acme/web", 42.into())
    }

    fn want_pr() -> WebhookEvent {
        WebhookEvent::PullRequest {
            installation: "42".into(),
            repo: "acme__web".into(),
            pr: 7,
        }
    }

    #[rstest]
    #[case::positive_opened("pull_request", pr("opened"), Ok(want_pr()))]
    #[case::positive_synchronize("pull_request", pr("synchronize"), Ok(want_pr()))]
    #[case::positive_reopened("pull_request", pr("reopened"), Ok(want_pr()))]
    #[case::positive_ready_for_review("pull_request", pr("ready_for_review"), Ok(want_pr()))]
    #[case::positive_ping("ping", b"not even json".to_vec(), Ok(WebhookEvent::Ping))]
    #[case::negative_closed_ignored("pull_request", pr("closed"), Ok(WebhookEvent::Ignored))]
    #[case::negative_unknown_event_ignored("push", b"{}".to_vec(), Ok(WebhookEvent::Ignored))]
    #[case::negative_draft_ignored("pull_request", pr_body("opened", 7.into(), true.into(), "acme/web", 42.into()), Ok(WebhookEvent::Ignored))]
    #[case::corner_draft_null_is_not_draft("pull_request", pr_body("opened", 7.into(), Value::Null, "acme/web", 42.into()), Ok(want_pr()))]
    #[case::corner_repo_name_with_double_underscore("pull_request", pr_body("opened", 7.into(), false.into(), "acme/my__web", 42.into()), Ok(WebhookEvent::PullRequest { installation: "42".into(), repo: "acme__my__web".into(), pr: 7 }))]
    #[case::boundary_pr_max("pull_request", pr_body("opened", u32::MAX.into(), false.into(), "acme/web", 42.into()), Ok(WebhookEvent::PullRequest { installation: "42".into(), repo: "acme__web".into(), pr: u64::from(u32::MAX) }))]
    #[case::boundary_pr_over_max("pull_request", pr_body("opened", (u64::from(u32::MAX) + 1).into(), false.into(), "acme/web", 42.into()), Err(Reject("pull request number out of range")))]
    #[case::boundary_pr_zero("pull_request", pr_body("opened", 0.into(), false.into(), "acme/web", 42.into()), Err(Reject("pull request number out of range")))]
    #[case::adversarial_pr_negative("pull_request", pr_body("opened", (-7).into(), false.into(), "acme/web", 42.into()), Err(Reject("pull request number out of range")))]
    #[case::adversarial_pr_string("pull_request", pr_body("opened", "7".into(), false.into(), "acme/web", 42.into()), Err(Reject("pull request number out of range")))]
    #[case::adversarial_pr_float("pull_request", pr_body("opened", 7.5.into(), false.into(), "acme/web", 42.into()), Err(Reject("pull request number out of range")))]
    #[case::adversarial_draft_string("pull_request", pr_body("opened", 7.into(), "no".into(), "acme/web", 42.into()), Err(Reject("pull_request.draft is not a bool")))]
    #[case::adversarial_repo_traversal("pull_request", pr_body("opened", 7.into(), false.into(), "acme/..", 42.into()), Err(Reject("repository name is not a valid owner/name")))]
    #[case::adversarial_repo_extra_segment("pull_request", pr_body("opened", 7.into(), false.into(), "acme/web/../x", 42.into()), Err(Reject("repository name is not a valid owner/name")))]
    #[case::adversarial_owner_with_underscore("pull_request", pr_body("opened", 7.into(), false.into(), "ac_me/web", 42.into()), Err(Reject("repository name is not a valid owner/name")))]
    #[case::adversarial_owner_leading_dash("pull_request", pr_body("opened", 7.into(), false.into(), "-acme/web", 42.into()), Err(Reject("repository name is not a valid owner/name")))]
    #[case::adversarial_repo_leading_dash("pull_request", pr_body("opened", 7.into(), false.into(), "acme/-rf", 42.into()), Err(Reject("repository name is not a valid owner/name")))]
    #[case::adversarial_repo_name_too_long("pull_request", pr_body("opened", 7.into(), false.into(), &format!("acme/{}", "a".repeat(MAX_REPO_NAME_LEN + 1)), 42.into()), Err(Reject("repository name is not a valid owner/name")))]
    #[case::adversarial_installation_string("pull_request", pr_body("opened", 7.into(), false.into(), "acme/web", "42".into()), Err(Reject("missing or invalid installation id")))]
    #[case::adversarial_installation_negative("pull_request", pr_body("opened", 7.into(), false.into(), "acme/web", (-1).into()), Err(Reject("missing or invalid installation id")))]
    #[case::adversarial_not_json("pull_request", b"{\"action\":".to_vec(), Err(Reject("body is not JSON")))]
    #[case::adversarial_no_action("installation", b"{\"installation\":{\"id\":1}}".to_vec(), Err(Reject("missing action")))]
    fn parse_pull_request_cases(
        #[case] event: &str,
        #[case] body: Vec<u8>,
        #[case] want: std::result::Result<WebhookEvent, Reject>,
    ) {
        assert_eq!(parse(event, &body), want);
    }

    fn inst(action: &str, id: Value) -> Vec<u8> {
        serde_json::json!({ "action": action, "installation": { "id": id, "account": { "login": "acme" } } })
            .to_string()
            .into_bytes()
    }

    #[rstest]
    #[case::positive_deleted(inst("deleted", 42.into()), Ok(WebhookEvent::InstallDeleted { installation: "42".into() }))]
    #[case::positive_suspend(inst("suspend", 42.into()), Ok(WebhookEvent::InstallSuspended { installation: "42".into() }))]
    #[case::positive_unsuspend(inst("unsuspend", 42.into()), Ok(WebhookEvent::InstallUnsuspended { installation: "42".into() }))]
    #[case::negative_created_never_binds(inst("created", 42.into()), Ok(WebhookEvent::Ignored))]
    #[case::negative_permissions_ignored(inst("new_permissions_accepted", 42.into()), Ok(WebhookEvent::Ignored))]
    #[case::boundary_u64_max_id(inst("deleted", u64::MAX.into()), Ok(WebhookEvent::InstallDeleted { installation: u64::MAX.to_string() }))]
    #[case::adversarial_missing_installation(br#"{"action":"deleted"}"#.to_vec(), Err(Reject("missing or invalid installation id")))]
    #[case::adversarial_id_object(inst("deleted", serde_json::json!({"x": 1})), Err(Reject("missing or invalid installation id")))]
    fn parse_installation_cases(
        #[case] body: Vec<u8>,
        #[case] want: std::result::Result<WebhookEvent, Reject>,
    ) {
        assert_eq!(parse("installation", &body), want);
    }

    fn repos(action: &str, added: Value, removed: Value) -> Vec<u8> {
        serde_json::json!({ "action": action, "installation": { "id": 42 },
            "repositories_added": added, "repositories_removed": removed })
        .to_string()
        .into_bytes()
    }

    fn names(n: &[&str]) -> Value {
        Value::Array(
            n.iter()
                .map(|f| serde_json::json!({ "full_name": f }))
                .collect(),
        )
    }

    #[rstest]
    #[case::positive_added(repos("added", names(&["acme/a", "acme/b"]), names(&[])), Ok((vec!["acme__a", "acme__b"], vec![])))]
    #[case::positive_removed(repos("removed", names(&[]), names(&["acme/a"])), Ok((vec![], vec!["acme__a"])))]
    #[case::corner_lists_absent(br#"{"action":"added","installation":{"id":42}}"#.to_vec(), Ok((vec![], vec![])))]
    #[case::boundary_list_at_cap(repos("added", names(&vec!["acme/a"; MAX_SELECTED_REPOS]), names(&[])), Ok((vec!["acme__a"; MAX_SELECTED_REPOS], vec![])))]
    #[case::boundary_list_over_cap(repos("added", names(&vec!["acme/a"; MAX_SELECTED_REPOS + 1]), names(&[])), Err(Reject("repository list over the cap")))]
    #[case::adversarial_one_bad_entry_rejects_all(repos("added", names(&["acme/a", "acme/../../x"]), names(&[])), Err(Reject("repository name is not a valid owner/name")))]
    #[case::adversarial_list_not_array(repos("added", "acme/a".into(), names(&[])), Err(Reject("repository list is not a list")))]
    #[case::adversarial_entry_without_name(repos("added", serde_json::json!([{"id": 1}]), names(&[])), Err(Reject("repository name is not a valid owner/name")))]
    fn parse_repos_cases(
        #[case] body: Vec<u8>,
        #[case] want: std::result::Result<(Vec<&str>, Vec<&str>), Reject>,
    ) {
        let got = parse("installation_repositories", &body);
        let want = want.map(|(a, r)| WebhookEvent::ReposChanged {
            installation: "42".into(),
            added: a.into_iter().map(String::from).collect(),
            removed: r.into_iter().map(String::from).collect(),
        });
        assert_eq!(got, want);
    }

    #[test]
    fn negative_repos_other_action_ignored() {
        assert_eq!(
            parse(
                "installation_repositories",
                &repos("weird", names(&[]), names(&[]))
            ),
            Ok(WebhookEvent::Ignored)
        );
    }

    /// A fake fleet: installs owned per tenant, per-tenant rows, and a log of every action.
    #[derive(Default)]
    struct Fake {
        owners: HashMap<String, String>,
        rows: HashMap<String, Vec<FleetSession>>,
        log: Mutex<Vec<String>>,
        triggers: Mutex<Vec<FleetTrigger>>,
    }

    #[async_trait]
    impl WebhookTarget for Fake {
        async fn owner_of(&self, installation: &str) -> Result<Option<String>> {
            Ok(self.owners.get(installation).cloned())
        }
        async fn github_rows(&self, tenant: &str) -> Result<Vec<FleetSession>> {
            Ok(self.rows.get(tenant).cloned().unwrap_or_default())
        }
        fn enqueue(&self, trigger: FleetTrigger) -> TriggerOutcome {
            self.triggers.lock().unwrap().push(trigger);
            TriggerOutcome::Accepted
        }
        async fn revoke(&self, tenant: &str, installation: &str) -> Result<()> {
            self.log
                .lock()
                .unwrap()
                .push(format!("revoke {tenant} {installation}"));
            Ok(())
        }
        async fn suspend(&self, tenant: &str, installation: &str) -> Result<()> {
            self.log
                .lock()
                .unwrap()
                .push(format!("suspend {tenant} {installation}"));
            Ok(())
        }
        async fn unsuspend(&self, tenant: &str, installation: &str) -> Result<()> {
            self.log
                .lock()
                .unwrap()
                .push(format!("unsuspend {tenant} {installation}"));
            Ok(())
        }
        async fn sync_repos(
            &self,
            tenant: &str,
            installation: &str,
            added: &[String],
            removed: &[String],
        ) -> Result<()> {
            self.log.lock().unwrap().push(format!(
                "repos {tenant} {installation} +{} -{}",
                added.join(","),
                removed.join(",")
            ));
            Ok(())
        }
    }

    fn row(id: &str, repo: &str, enabled: bool) -> FleetSession {
        FleetSession {
            id: id.into(),
            repo: repo.into(),
            enabled,
            ..Default::default()
        }
    }

    fn fake() -> Fake {
        Fake {
            owners: HashMap::from([("42".into(), "acme".into()), ("99".into(), "globex".into())]),
            rows: HashMap::from([
                (
                    "acme".into(),
                    vec![
                        row("web", "acme__web", true),
                        row("web-sec", "acme__web", true),
                        row("off", "acme__web", false),
                        row("api", "acme__api", true),
                    ],
                ),
                ("globex".into(), vec![row("web", "acme__web", true)]),
            ]),
            ..Default::default()
        }
    }

    fn pr_event(installation: &str, repo: &str) -> WebhookEvent {
        WebhookEvent::PullRequest {
            installation: installation.into(),
            repo: repo.into(),
            pr: 7,
        }
    }

    #[rstest]
    #[case::positive_owner_rows_for_repo(pr_event("42", "acme__web"), &[("acme", "web"), ("acme", "web-sec")])]
    #[case::negative_no_matching_repo(pr_event("42", "acme__nope"), &[])]
    #[case::negative_unclaimed_install(pr_event("7", "acme__web"), &[])]
    #[case::adversarial_other_tenants_install_only_reaches_its_owner(pr_event("99", "acme__web"), &[("globex", "web")])]
    #[tokio::test]
    async fn dispatch_pull_request_routes_to_owner_tenant(
        #[case] event: WebhookEvent,
        #[case] want: &[(&str, &str)],
    ) {
        let f = fake();
        dispatch(event, &f).await.unwrap();
        let got: Vec<(String, String)> = f
            .triggers
            .lock()
            .unwrap()
            .iter()
            .map(|t| (t.tenant.clone(), t.session_id.clone()))
            .collect();
        let want: Vec<(String, String)> = want
            .iter()
            .map(|(t, s)| (t.to_string(), s.to_string()))
            .collect();
        assert_eq!(got, want);
        for t in f.triggers.lock().unwrap().iter() {
            assert_eq!(
                (t.pr_number, t.requested_by.as_slice()),
                (7, [WEBHOOK_REQUESTER.to_string()].as_slice())
            );
        }
    }

    #[rstest]
    #[case::positive_deleted_revokes_owner(WebhookEvent::InstallDeleted { installation: "42".into() }, Dispatched::Revoked, &["revoke acme 42"])]
    #[case::positive_suspend(WebhookEvent::InstallSuspended { installation: "99".into() }, Dispatched::Suspended, &["suspend globex 99"])]
    #[case::positive_unsuspend(WebhookEvent::InstallUnsuspended { installation: "42".into() }, Dispatched::Unsuspended, &["unsuspend acme 42"])]
    #[case::positive_repos(WebhookEvent::ReposChanged { installation: "42".into(), added: vec!["acme__a".into()], removed: vec!["acme__b".into()] }, Dispatched::ReposSynced, &["repos acme 42 +acme__a -acme__b"])]
    #[case::negative_unclaimed_delete_does_nothing(WebhookEvent::InstallDeleted { installation: "7".into() }, Dispatched::Unclaimed, &[])]
    #[case::negative_unclaimed_unsuspend_never_binds(WebhookEvent::InstallUnsuspended { installation: "7".into() }, Dispatched::Unclaimed, &[])]
    #[case::corner_ping(WebhookEvent::Ping, Dispatched::Pong, &[])]
    #[case::corner_ignored(WebhookEvent::Ignored, Dispatched::Ignored, &[])]
    #[tokio::test]
    async fn dispatch_lifecycle_cases(
        #[case] event: WebhookEvent,
        #[case] want: Dispatched,
        #[case] log: &[&str],
    ) {
        let f = fake();
        assert_eq!(dispatch(event, &f).await.unwrap(), want);
        assert_eq!(f.log.lock().unwrap().as_slice(), log);
        assert!(f.triggers.lock().unwrap().is_empty());
    }
}
