//! `agent fleet …` — operator verbs over a running fleet's `ReviewFleetService`.
//!
//! `skip` lists the review drafts that are still open (not posted, not skipped),
//! prints them, asks, and skips them with `SkipReviews`. Only the ids it listed and
//! printed are sent, so a draft that lands after the listing is never skipped
//! unseen. It never approves or posts anything.
//!
//! The fleet is dialed at `[grpc.fleet] endpoint` (default the `FLEET` port on
//! loopback), carrying the `[grpc.client] bearer` credential (e.g. the stored
//! `agent login` token) when auth is on. Everything the server sends back is
//! untrusted: repo names are cleaned of control characters before printing.

use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::time::Duration;

use agent_core::{draft_status, ReviewDraftFilter, ReviewDraftRecord, SkipOutcome};
use anyhow::{bail, Context, Result};

use crate::config::Config;

pub const USAGE: &str = "usage: agent [--config PATH] fleet <verb> …

  skip [--repo OWNER/NAME] [--status drafted|approved|superseded] [--dry-run] [--yes]
      Skip the open review drafts (default: every status but posted and skipped),
      after listing them and asking. --dry-run lists only; --yes skips without asking.
      Nothing is approved or posted.

The fleet is dialed at [grpc.fleet] endpoint (default 127.0.0.1:50086).";

/// `fleet skip`, validated.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkipArgs {
    /// Roster repo key (`owner__name`), from `--repo owner/name`.
    pub repo: Option<String>,
    /// Only drafts in this status.
    pub status: Option<String>,
    pub dry_run: bool,
    pub yes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FleetCmd {
    Skip(SkipArgs),
    Help,
}

/// The statuses `--status` may name: the ones a skip can still change.
const SKIPPABLE: [&str; 3] = [
    draft_status::DRAFTED,
    draft_status::APPROVED,
    draft_status::SUPERSEDED,
];

fn echo(s: &str) -> String {
    clean(
        &s.chars()
            .take(agent_core::MAX_ECHO_CHARS)
            .collect::<String>(),
    )
}

/// `s` with every control character (ANSI escapes, newlines) and every hidden or bidi
/// control (zero-width, overrides, isolates) replaced by `?`, so server text can neither
/// drive the terminal nor reorder what the operator reads.
fn clean(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control() || agent_core::is_hidden_control(c) {
                '?'
            } else {
                c
            }
        })
        .collect()
}

/// `owner/name` → the roster key `owner__name`. Each half is a GitHub-style name.
fn repo_key(slug: &str) -> Result<String> {
    let ok = |p: &str| {
        !p.is_empty()
            && p.len() <= 100
            && !p.starts_with('.')
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    match slug.split_once('/') {
        Some((owner, name)) if ok(owner) && ok(name) && !name.contains('/') => {
            Ok(format!("{owner}__{name}"))
        }
        _ => bail!("--repo `{}` is not OWNER/NAME", echo(slug)),
    }
}

/// Parse the words after `fleet`.
pub fn parse(args: &[String]) -> Result<FleetCmd> {
    let mut it = args.iter();
    let Some(verb) = it.next() else {
        return Ok(FleetCmd::Help);
    };
    match verb.as_str() {
        "help" | "--help" | "-h" => Ok(FleetCmd::Help),
        "skip" => {
            let mut a = SkipArgs::default();
            while let Some(flag) = it.next() {
                match flag.as_str() {
                    "--repo" => {
                        let v = it.next().context("--repo requires OWNER/NAME")?;
                        a.repo = Some(repo_key(v)?);
                    }
                    "--status" => {
                        let v = it.next().context("--status requires a status")?;
                        if !SKIPPABLE.contains(&v.as_str()) {
                            bail!(
                                "--status `{}` is not one of {}",
                                echo(v),
                                SKIPPABLE.join("|")
                            );
                        }
                        a.status = Some(v.clone());
                    }
                    "--dry-run" => a.dry_run = true,
                    "--yes" => a.yes = true,
                    other => bail!("fleet skip: unknown argument `{}`", echo(other)),
                }
            }
            Ok(FleetCmd::Skip(a))
        }
        other => bail!(
            "unknown fleet verb `{}` (try `agent fleet help`)",
            echo(other)
        ),
    }
}

/// The `ListReviews` filter for `skip`: the asked-for status, or everything still open.
pub fn skip_filter(a: &SkipArgs) -> ReviewDraftFilter {
    ReviewDraftFilter {
        repo: a.repo.clone(),
        session_id: None,
        status: a.status.clone(),
        exclude_status: vec![
            draft_status::POSTED.to_string(),
            draft_status::SKIPPED.to_string(),
        ],
        limit: agent_core::MAX_DRAFT_ROWS,
    }
}

/// The ids to skip from a listing: every draft the filter returned, minus any the
/// server shouldn't have sent (a posted or skipped one, or one with a malformed id).
pub fn skip_targets(rows: &[ReviewDraftRecord]) -> Vec<String> {
    rows.iter()
        .filter(|r| r.status != draft_status::POSTED && r.status != draft_status::SKIPPED)
        .filter(|r| agent_core::valid_review_id(&r.review_id))
        .map(|r| r.review_id.clone())
        .collect()
}

/// One line per draft: repo, PR, status, id.
fn render(rows: &[ReviewDraftRecord], targets: &[String]) -> String {
    let targets: HashSet<&str> = targets.iter().map(String::as_str).collect();
    let mut out = String::new();
    for r in rows
        .iter()
        .filter(|r| targets.contains(r.review_id.as_str()))
    {
        out.push_str(&format!(
            "  {}#{}  {}  {}\n",
            clean(&r.repo.replacen("__", "/", 1)),
            r.pr_number,
            clean(&r.status),
            r.review_id
        ));
    }
    out
}

/// A one-line tally of skip outcomes, e.g. `3 skipped, 1 already posted`.
pub fn tally(results: &[(String, SkipOutcome)]) -> String {
    let count = |o: SkipOutcome| results.iter().filter(|(_, r)| *r == o).count();
    [
        (count(SkipOutcome::Skipped), "skipped"),
        (count(SkipOutcome::AlreadySkipped), "already skipped"),
        (count(SkipOutcome::AlreadyPosted), "already posted"),
        (count(SkipOutcome::NotFound), "not found"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, what)| format!("{n} {what}"))
    .collect::<Vec<_>>()
    .join(", ")
}

/// What `fleet skip` does once it has listed `n` open drafts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Nothing is open: no prompt, no call.
    Nothing,
    /// `--dry-run`: print the list, skip nothing.
    DryRun,
    /// Print the list and ask before skipping.
    Ask,
    /// `--yes`: skip without asking.
    Skip,
}

/// The step for `a` with `n` open drafts. `--dry-run` wins over `--yes`.
pub fn step(a: &SkipArgs, n: usize) -> Step {
    match (n, a.dry_run, a.yes) {
        (0, _, _) => Step::Nothing,
        (_, true, _) => Step::DryRun,
        (_, false, true) => Step::Skip,
        (_, false, false) => Step::Ask,
    }
}

/// Read the answer to the skip prompt: only the exact word `yes` (surrounding
/// whitespace aside) confirms. Anything else, including `YES` and end of input,
/// skips nothing.
pub fn confirmed(input: &mut impl BufRead) -> Result<bool> {
    let mut answer = String::new();
    let n = input.read_line(&mut answer)?;
    Ok(n > 0 && answer.trim() == "yes")
}

/// How long `fleet skip` waits for `SkipReviews` (up to [`agent_core::MAX_SKIP_BATCH`]
/// lease-guarded skips, one after another).
const SKIP_DEADLINE: Duration = Duration::from_secs(300);

/// The two `ReviewFleetService` calls `fleet skip` makes, so its whole flow can be
/// driven by a fake in tests.
#[async_trait::async_trait]
pub trait SkipFleet {
    async fn list(&self, filter: &ReviewDraftFilter) -> Result<Vec<ReviewDraftRecord>>;
    async fn skip(&self, ids: &[String]) -> Result<Vec<(String, SkipOutcome)>>;
}

#[async_trait::async_trait]
impl SkipFleet for agent_grpc::client::GrpcFleet {
    async fn list(&self, filter: &ReviewDraftFilter) -> Result<Vec<ReviewDraftRecord>> {
        Ok(self.list_reviews(filter).await?)
    }
    async fn skip(&self, ids: &[String]) -> Result<Vec<(String, SkipOutcome)>> {
        Ok(self.skip_reviews(ids).await?)
    }
}

/// Run a parsed `agent fleet …` verb against the configured fleet, returning what to
/// print. Parse with [`parse`] first, so a bad verb fails before the config is needed.
pub async fn execute(cfg: &Config, cmd: FleetCmd) -> Result<String> {
    let a = match cmd {
        FleetCmd::Help => return Ok(USAGE.to_string()),
        FleetCmd::Skip(a) => a,
    };
    #[cfg(feature = "auth")]
    crate::login::install_client_bearer(cfg)?;
    let ep = crate::registry::grpc_client_endpoint(
        &cfg.grpc.fleet.endpoint,
        agent_grpc::constants::FLEET,
    );
    let client = agent_grpc::client::GrpcFleet::connect(&ep)?;
    let mut stdout = std::io::stdout();
    run_skip(
        &client,
        &a,
        &mut std::io::stdin().lock(),
        &mut stdout,
        SKIP_DEADLINE,
    )
    .await
}

/// `fleet skip`: list, print, ask (on `prompt`, reading `input`), skip, tally. Only the
/// ids it listed are sent.
pub async fn run_skip(
    fleet: &(impl SkipFleet + Sync),
    a: &SkipArgs,
    input: &mut impl BufRead,
    prompt: &mut impl Write,
    deadline: Duration,
) -> Result<String> {
    let rows = fleet
        .list(&skip_filter(a))
        .await
        .context("listing review drafts")?;
    let targets = skip_targets(&rows);
    // The listing has no cursor: a full page may have more behind it.
    let more = if rows.len() >= agent_core::MAX_DRAFT_ROWS {
        format!(
            "\n(the listing stops at {}; run again for the rest)",
            agent_core::MAX_DRAFT_ROWS
        )
    } else {
        String::new()
    };
    let listing = format!(
        "{} open review draft(s):\n{}{more}",
        targets.len(),
        render(&rows, &targets)
    );
    match step(a, targets.len()) {
        Step::Nothing => return Ok("no open review drafts to skip".into()),
        Step::DryRun => return Ok(format!("{listing}\n--dry-run: nothing skipped")),
        Step::Ask => {
            write!(
                prompt,
                "{listing}\nSkip all {}? Type `yes` to continue: ",
                targets.len()
            )?;
            prompt.flush()?;
            if !confirmed(input)? {
                return Ok("not confirmed: nothing skipped".into());
            }
        }
        Step::Skip => {}
    }
    // One call: a whole listing fits (MAX_DRAFT_ROWS <= MAX_SKIP_BATCH, asserted in agent-core).
    let results = tokio::time::timeout(deadline, fleet.skip(&targets))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "no answer from SkipReviews within {}s; some drafts may be skipped. \
                 Run `agent fleet skip --dry-run` to see what is left",
                deadline.as_secs()
            )
        })?
        .context(
            "skipping review drafts (the ones skipped before the failure stay skipped; \
             run again to finish)",
        )?;
    Ok(format!("{}{more}", tally(&results)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn words(v: &[&str]) -> Vec<String> {
        v.iter().map(ToString::to_string).collect()
    }

    /// `agent fleet …` parsing: name, description, words, expected (or an error fragment).
    #[rstest]
    #[case::positive_bare_skip("skip with no flags skips everything open", &["skip"], Ok(FleetCmd::Skip(SkipArgs::default())))]
    #[case::positive_all_flags(
        "every flag together",
        &["skip", "--repo", "runpod/host", "--status", "drafted", "--dry-run", "--yes"],
        Ok(FleetCmd::Skip(SkipArgs {
            repo: Some("runpod__host".into()),
            status: Some("drafted".into()),
            dry_run: true,
            yes: true,
        }))
    )]
    #[case::corner_no_verb("no verb prints the usage", &[], Ok(FleetCmd::Help))]
    #[case::corner_help("help prints the usage", &["--help"], Ok(FleetCmd::Help))]
    #[case::corner_dotted_repo(
        "a repo name may hold dots",
        &["skip", "--repo", "NixOS/nixpkgs.dev"],
        Ok(FleetCmd::Skip(SkipArgs { repo: Some("NixOS__nixpkgs.dev".into()), ..SkipArgs::default() }))
    )]
    #[case::negative_unknown_verb("an unknown verb is refused", &["approve"], Err("unknown fleet verb"))]
    #[case::negative_unknown_flag("an unknown flag is refused", &["skip", "--all"], Err("unknown argument"))]
    #[case::negative_posted_status("a posted draft can't be skipped", &["skip", "--status", "posted"], Err("not one of"))]
    #[case::negative_skipped_status("a skipped draft can't be skipped again", &["skip", "--status", "skipped"], Err("not one of"))]
    #[case::boundary_repo_missing_value("--repo needs a value", &["skip", "--repo"], Err("requires OWNER/NAME"))]
    #[case::boundary_status_missing_value("--status needs a value", &["skip", "--status"], Err("requires a status"))]
    #[case::boundary_repo_no_slash("a repo without an owner is refused", &["skip", "--repo", "host"], Err("not OWNER/NAME"))]
    #[case::adversarial_repo_traversal("a traversal is no repo", &["skip", "--repo", "../etc"], Err("not OWNER/NAME"))]
    #[case::adversarial_repo_extra_slash("a nested path is no repo", &["skip", "--repo", "a/b/c"], Err("not OWNER/NAME"))]
    #[case::adversarial_repo_double_underscore_injection(
        "a key smuggled in directly is still checked as OWNER/NAME",
        &["skip", "--repo", "a__b"],
        Err("not OWNER/NAME")
    )]
    #[case::adversarial_escape_in_error(
        "an escape sequence in a bad flag is not echoed raw",
        &["skip", "\u{1b}[2J"],
        Err("unknown argument `?[2J`")
    )]
    fn parse_cases(
        #[case] description: &str,
        #[case] args: &[&str],
        #[case] expected: std::result::Result<FleetCmd, &str>,
    ) {
        match (parse(&words(args)), expected) {
            (Ok(got), Ok(want)) => assert_eq!(got, want, "{description}"),
            (Err(e), Err(frag)) => {
                let e = e.to_string();
                assert!(e.contains(frag), "{description}: {e:?} lacks {frag:?}");
                assert!(!e.contains('\u{1b}'), "{description}: raw escape echoed");
            }
            (got, want) => panic!("{description}: got {got:?}, want {want:?}"),
        }
    }

    /// The listing filter: name, description, args, expected status and exclusions.
    #[rstest]
    #[case::positive_default_all_open(
        "by default every open status is listed",
        SkipArgs::default(),
        None
    )]
    #[case::positive_one_status(
        "--status narrows the listing",
        SkipArgs { status: Some("superseded".into()), ..SkipArgs::default() },
        Some("superseded")
    )]
    fn skip_filter_cases(
        #[case] description: &str,
        #[case] args: SkipArgs,
        #[case] status: Option<&str>,
    ) {
        let f = skip_filter(&args);
        assert_eq!(f.status.as_deref(), status, "{description}");
        assert_eq!(
            f.exclude_status,
            vec!["posted", "skipped"],
            "{description}: never lists finished drafts"
        );
        assert_eq!(f.limit, agent_core::MAX_DRAFT_ROWS, "{description}");
    }

    fn row(id: &str, status: &str) -> ReviewDraftRecord {
        ReviewDraftRecord {
            review_id: id.into(),
            repo: "o__r".into(),
            pr_number: 1,
            head_sha: String::new(),
            risk_score: 0.0,
            gate_failed: false,
            n_findings: 0,
            files_changed: 0,
            additions: 0,
            deletions: 0,
            draft_path: String::new(),
            status: status.into(),
            requested_by: Vec::new(),
            approved_by: String::new(),
        }
    }

    /// Which listed drafts are sent: name, description, rows, expected ids.
    #[rstest]
    #[case::positive_open_drafts("drafted, approved and superseded are all sent", vec![row("a", "drafted"), row("b", "approved"), row("c", "superseded")], &["a", "b", "c"])]
    #[case::corner_empty("an empty listing sends nothing", vec![], &[])]
    #[case::negative_finished_rows_dropped(
        "a posted or skipped row the server shouldn't have sent is dropped",
        vec![row("a", "posted"), row("b", "skipped"), row("c", "drafted")],
        &["c"]
    )]
    #[case::boundary_order_kept("listing order is kept", vec![row("z", "drafted"), row("a", "drafted")], &["z", "a"])]
    #[case::adversarial_bad_id_dropped(
        "a malformed id from the server is never sent",
        vec![row("../x", "drafted"), row("ok", "drafted")],
        &["ok"]
    )]
    fn skip_targets_cases(
        #[case] description: &str,
        #[case] rows: Vec<ReviewDraftRecord>,
        #[case] expected: &[&str],
    ) {
        assert_eq!(skip_targets(&rows), words(expected), "{description}");
    }

    /// The outcome summary: name, description, outcomes, expected line.
    #[rstest]
    #[case::positive_all_skipped("every id skipped", vec![SkipOutcome::Skipped, SkipOutcome::Skipped], "2 skipped")]
    #[case::positive_mixed(
        "each outcome counted, in a fixed order",
        vec![SkipOutcome::NotFound, SkipOutcome::Skipped, SkipOutcome::AlreadyPosted],
        "1 skipped, 1 already posted, 1 not found"
    )]
    #[case::corner_none("no results is an empty line", vec![], "")]
    #[case::boundary_only_repeats("only repeats", vec![SkipOutcome::AlreadySkipped], "1 already skipped")]
    fn tally_cases(
        #[case] description: &str,
        #[case] outcomes: Vec<SkipOutcome>,
        #[case] expected: &str,
    ) {
        let results: Vec<(String, SkipOutcome)> = outcomes
            .into_iter()
            .enumerate()
            .map(|(i, o)| (format!("r{i}"), o))
            .collect();
        assert_eq!(tally(&results), expected, "{description}");
    }

    /// Server text is cleaned before printing: an escape in a repo name can't reach the terminal.
    #[rstest]
    #[case::adversarial_escape_in_repo("an ANSI escape in a repo name", "o__r\u{1b}[2J", "o/r?[2J")]
    #[case::adversarial_newline_in_repo(
        "a newline can't forge a second line",
        "o__r\nfake",
        "o/r?fake"
    )]
    #[case::positive_plain_repo("a plain repo prints as OWNER/NAME", "o__r", "o/r")]
    #[case::adversarial_bidi_override_in_repo(
        "a right-to-left override can't reverse what the operator reads",
        "o__r\u{202e}evif",
        "o/r?evif"
    )]
    #[case::adversarial_zero_width_in_repo(
        "a zero-width space is shown, not hidden",
        "o__r\u{200b}x",
        "o/r?x"
    )]
    #[case::adversarial_bidi_isolate_in_repo(
        "a bidi isolate is shown, not obeyed",
        "o__\u{2066}r\u{2069}",
        "o/?r?"
    )]
    fn render_cleans(#[case] description: &str, #[case] repo: &str, #[case] shown: &str) {
        let mut r = row("a", "drafted");
        r.repo = repo.into();
        let out = render(&[r], &words(&["a"]));
        assert!(out.contains(shown), "{description}: {out:?}");
        assert!(!out.contains('\u{1b}'), "{description}");
        assert!(
            !out.chars().any(agent_core::is_hidden_control),
            "{description}: {out:?}"
        );
        assert_eq!(out.lines().count(), 1, "{description}");
    }

    fn args(dry_run: bool, yes: bool) -> SkipArgs {
        SkipArgs {
            dry_run,
            yes,
            ..SkipArgs::default()
        }
    }

    /// The confirm gate: description, flags, open drafts, expected step. Only `Step::Skip`
    /// and a confirmed `Step::Ask` reach `SkipReviews`.
    #[rstest]
    #[case::positive_asks_by_default("no flags: list, then ask", args(false, false), 3, Step::Ask)]
    #[case::positive_yes_skips("--yes skips without asking", args(false, true), 3, Step::Skip)]
    #[case::positive_dry_run_lists("--dry-run lists only", args(true, false), 3, Step::DryRun)]
    #[case::corner_dry_run_beats_yes(
        "--dry-run --yes still skips nothing",
        args(true, true),
        3,
        Step::DryRun
    )]
    #[case::boundary_nothing_open(
        "no open drafts: no prompt, no call",
        args(false, true),
        0,
        Step::Nothing
    )]
    #[case::boundary_one_open(
        "a single open draft is still asked about",
        args(false, false),
        1,
        Step::Ask
    )]
    #[case::boundary_full_listing(
        "a full listing is one skip",
        args(false, true),
        agent_core::MAX_DRAFT_ROWS,
        Step::Skip
    )]
    fn step_cases(
        #[case] description: &str,
        #[case] a: SkipArgs,
        #[case] n: usize,
        #[case] expected: Step,
    ) {
        assert_eq!(step(&a, n), expected, "{description}");
    }

    /// The prompt's answer: description, typed input, whether it confirms.
    #[rstest]
    #[case::positive_yes("`yes` confirms", "yes\n", true)]
    #[case::corner_padded_yes("surrounding whitespace is ignored", "  yes \r\n", true)]
    #[case::corner_yes_without_newline("`yes` then end of input confirms", "yes", true)]
    #[case::negative_no("`no` skips nothing", "no\n", false)]
    #[case::negative_y("`y` is not enough", "y\n", false)]
    #[case::negative_empty_line("a bare Enter skips nothing", "\n", false)]
    #[case::boundary_eof("end of input (stdin closed) fails closed", "", false)]
    #[case::adversarial_upper_case("`YES` is not the word asked for", "YES\n", false)]
    #[case::adversarial_yes_prefix("`yesterday` is not `yes`", "yesterday\n", false)]
    #[case::adversarial_second_line("only the first line counts", "no\nyes\n", false)]
    fn confirmed_cases(#[case] description: &str, #[case] input: &str, #[case] expected: bool) {
        let mut r = std::io::Cursor::new(input.as_bytes());
        assert_eq!(confirmed(&mut r).unwrap(), expected, "{description}");
    }

    /// A fleet double: serves `rows` (or a list error), records each skip call, and
    /// answers every asked id `skipped` after `delay`.
    struct FakeFleet {
        rows: Option<Vec<ReviewDraftRecord>>,
        delay: Duration,
        skipped: std::sync::Mutex<Vec<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl SkipFleet for FakeFleet {
        async fn list(&self, _: &ReviewDraftFilter) -> Result<Vec<ReviewDraftRecord>> {
            self.rows.clone().context("fleet unreachable")
        }
        async fn skip(&self, ids: &[String]) -> Result<Vec<(String, SkipOutcome)>> {
            self.skipped.lock().unwrap().push(ids.to_vec());
            tokio::time::sleep(self.delay).await;
            Ok(ids
                .iter()
                .map(|i| (i.clone(), SkipOutcome::Skipped))
                .collect())
        }
    }

    fn drafted(n: usize) -> Option<Vec<ReviewDraftRecord>> {
        Some((0..n).map(|i| row(&format!("r{i}"), "drafted")).collect())
    }

    /// The whole `fleet skip` flow: description, listing, flags, typed answer, expected
    /// output fragment (or error fragment), the skip calls' sizes, and whether a prompt
    /// was shown.
    #[rstest]
    #[case::positive_confirmed_skips_the_listed(
        "`yes` skips exactly the listed drafts",
        drafted(2), args(false, false), "yes\n", Duration::ZERO, (Ok("2 skipped"), vec![2], true)
    )]
    #[case::negative_declined_skips_nothing(
        "any other answer skips nothing",
        drafted(2), args(false, false), "no\n", Duration::ZERO, (Ok("not confirmed"), vec![], true)
    )]
    #[case::negative_eof_skips_nothing(
        "end of input is not a yes",
        drafted(2), args(false, false), "", Duration::ZERO, (Ok("not confirmed"), vec![], true)
    )]
    #[case::positive_dry_run_calls_no_skip(
        "--dry-run lists and stops",
        drafted(3), args(true, false), "", Duration::ZERO, (Ok("--dry-run: nothing skipped"), vec![], false)
    )]
    #[case::boundary_nothing_open(
        "an empty listing neither asks nor calls",
        drafted(0), args(false, false), "yes\n", Duration::ZERO, (Ok("no open review drafts"), vec![], false)
    )]
    #[case::corner_full_page_notes_more(
        "a full page skips it in one call and says more may follow",
        drafted(agent_core::MAX_DRAFT_ROWS), args(false, true), "", Duration::ZERO,
        (Ok("the listing stops at 500"), vec![agent_core::MAX_DRAFT_ROWS], false)
    )]
    #[case::negative_list_error(
        "a listing failure is reported, nothing skipped",
        None, args(false, true), "", Duration::ZERO, (Err("listing review drafts"), vec![], false)
    )]
    #[case::adversarial_stalled_skip_times_out(
        "a fleet that never answers the skip is cut off at the deadline",
        drafted(1), args(false, true), "", Duration::from_secs(3600),
        (Err("no answer from SkipReviews"), vec![1], false)
    )]
    #[tokio::test(start_paused = true)]
    async fn run_skip_cases(
        #[case] description: &str,
        #[case] rows: Option<Vec<ReviewDraftRecord>>,
        #[case] a: SkipArgs,
        #[case] typed: &str,
        #[case] delay: Duration,
        #[case] want: (std::result::Result<&str, &str>, Vec<usize>, bool),
    ) {
        let (expected, calls, asked) = want;
        let fleet = FakeFleet {
            rows,
            delay,
            skipped: std::sync::Mutex::new(Vec::new()),
        };
        let mut prompt = Vec::new();
        let got = run_skip(
            &fleet,
            &a,
            &mut typed.as_bytes(),
            &mut prompt,
            Duration::from_secs(300),
        )
        .await;
        match (got, expected) {
            (Ok(out), Ok(frag)) => assert!(out.contains(frag), "{description}: {out:?}"),
            (Err(e), Err(frag)) => {
                assert!(format!("{e:#}").contains(frag), "{description}: {e:#}");
            }
            (got, want) => panic!("{description}: got {got:?}, want {want:?}"),
        }
        let sizes: Vec<usize> = fleet.skipped.lock().unwrap().iter().map(Vec::len).collect();
        assert_eq!(sizes, calls, "{description}: skip calls");
        let shown = String::from_utf8(prompt).unwrap();
        assert_eq!(
            shown.contains("Type `yes`"),
            asked,
            "{description}: {shown:?}"
        );
    }
}
