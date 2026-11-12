//! Prometheus's scrape targets, from `/api/v1/targets?state=active`: every active
//! target should be `up`. A dashboard over a target that is down shows "No data", not
//! an error, so this is the check that tells the two apart.
//!
//! The reply is untrusted input: the target list is capped, every string is bounded
//! before it is printed, and a reply that is not Prometheus's shape fails closed.

use crate::{bounded, invalid, Result};
use serde_json::Value;

/// Most active targets read from one reply.
pub const MAX_TARGETS: usize = 2000;

/// One active scrape target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub job: String,
    pub instance: String,
    /// `up`, `down` or `unknown` (not scraped yet).
    pub health: String,
    pub last_error: String,
}

/// A `--allow-down` job name: 1–128 visible ASCII characters (a Prometheus job is a
/// label value, so this only keeps it printable and bounded).
pub fn valid_job(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_graphic())
}

/// The active targets in a `/api/v1/targets` reply.
pub fn parse_targets(v: &Value) -> Result<Vec<Target>> {
    if v.get("status").and_then(Value::as_str) != Some("success") {
        return Err(invalid("the reply's status is not \"success\""));
    }
    let list = v
        .pointer("/data/activeTargets")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("the reply has no data.activeTargets list"))?;
    if list.len() > MAX_TARGETS {
        return Err(invalid(format!("more than {MAX_TARGETS} targets")));
    }
    list.iter()
        .map(|t| {
            let s = |k: &str| t.pointer(k).and_then(Value::as_str);
            let job = s("/labels/job")
                .or_else(|| s("/scrapePool"))
                .ok_or_else(|| invalid("a target has no job"))?;
            let health = s("/health").ok_or_else(|| invalid("a target has no health"))?;
            Ok(Target {
                job: bounded(job),
                instance: bounded(s("/labels/instance").unwrap_or("")),
                health: bounded(health),
                last_error: bounded(s("/lastError").unwrap_or("")),
            })
        })
        .collect()
}

/// What the targets add up to: how many are up, the down ones that fail the check,
/// and the down ones `allow_down` names (reported, not failed). A target that has not
/// been scraped yet (`unknown`) fails: the check runs against a settled stack. No
/// target at all fails too, since then nothing is being scraped.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    pub up: usize,
    pub failed: Vec<String>,
    pub allowed: Vec<String>,
}

pub fn verdict(targets: &[Target], allow_down: &[String]) -> Verdict {
    let mut v = Verdict::default();
    if targets.is_empty() {
        v.failed.push("no active targets".into());
        return v;
    }
    for t in targets {
        if t.health == "up" {
            v.up += 1;
            continue;
        }
        let line = if t.last_error.is_empty() {
            format!("{} {} is {}", t.job, t.instance, t.health)
        } else {
            format!("{} {} is {}: {}", t.job, t.instance, t.health, t.last_error)
        };
        if allow_down.contains(&t.job) {
            v.allowed.push(line);
        } else {
            v.failed.push(line);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serde_json::json;

    fn target(job: &str, health: &str, err: &str) -> Value {
        json!({ "labels": { "job": job, "instance": "127.0.0.1:1" }, "scrapePool": job,
                "health": health, "lastError": err })
    }

    fn reply(targets: Vec<Value>) -> Value {
        json!({ "status": "success", "data": { "activeTargets": targets, "droppedTargets": [] } })
    }

    fn t(job: &str, health: &str, err: &str) -> Target {
        Target {
            job: job.into(),
            instance: "127.0.0.1:1".into(),
            health: health.into(),
            last_error: err.into(),
        }
    }

    #[rstest]
    #[case::positive(reply(vec![target("agent", "up", "")]), Ok(vec![t("agent", "up", "")]))]
    #[case::corner_job_from_scrape_pool(
        reply(vec![json!({ "scrapePool": "p", "health": "down", "labels": {} })]),
        Ok(vec![Target { job: "p".into(), instance: String::new(), health: "down".into(), last_error: String::new() }]))]
    #[case::boundary_no_targets(reply(vec![]), Ok(vec![]))]
    #[case::negative_error_status(json!({ "status": "error", "error": "x" }), Err("not \"success\""))]
    #[case::negative_no_list(json!({ "status": "success", "data": {} }), Err("no data.activeTargets"))]
    #[case::negative_no_health(reply(vec![json!({ "labels": { "job": "a" } })]), Err("no health"))]
    #[case::adversarial_not_an_object(json!("success"), Err("not \"success\""))]
    #[case::adversarial_too_many(reply(vec![target("a", "up", ""); MAX_TARGETS + 1]), Err("more than"))]
    #[case::adversarial_huge_error_is_bounded(
        reply(vec![target("a", "down", &"e".repeat(10_000))]),
        Ok(vec![t("a", "down", &bounded(&"e".repeat(10_000)))]))]
    fn parse_targets_cases(#[case] v: Value, #[case] want: std::result::Result<Vec<Target>, &str>) {
        match (parse_targets(&v), want) {
            (Ok(got), Ok(want)) => assert_eq!(got, want),
            (Err(e), Err(frag)) => assert!(e.to_string().contains(frag), "{e}"),
            (got, want) => panic!("got {got:?}, want {want:?}"),
        }
    }

    #[rstest]
    #[case::positive_all_up(vec![t("a", "up", ""), t("b", "up", "")], &[], 2, 0, 0)]
    #[case::negative_one_down(vec![t("a", "up", ""), t("b", "down", "refused")], &[], 1, 1, 0)]
    #[case::positive_down_but_allowed(vec![t("a", "up", ""), t("node", "down", "refused")], &["node"], 1, 0, 1)]
    #[case::negative_not_scraped_yet(vec![t("a", "unknown", "")], &[], 0, 1, 0)]
    #[case::boundary_no_targets(vec![], &[], 0, 1, 0)]
    #[case::corner_allow_is_exact(vec![t("nodes", "down", "")], &["node"], 0, 1, 0)]
    #[case::boundary_everything_allowed_still_needs_a_target(vec![], &["a"], 0, 1, 0)]
    fn verdict_cases(
        #[case] targets: Vec<Target>,
        #[case] allow: &[&str],
        #[case] up: usize,
        #[case] failed: usize,
        #[case] allowed: usize,
    ) {
        let allow: Vec<String> = allow.iter().map(ToString::to_string).collect();
        let v = verdict(&targets, &allow);
        assert_eq!(
            (v.up, v.failed.len(), v.allowed.len()),
            (up, failed, allowed),
            "{v:?}"
        );
    }

    #[rstest]
    #[case::positive("agent-gateway", true)]
    #[case::boundary_128(&"a".repeat(128), true)]
    #[case::boundary_129(&"a".repeat(129), false)]
    #[case::negative_empty("", false)]
    #[case::adversarial_space("a b", false)]
    #[case::adversarial_newline("a\nFAILED", false)]
    #[case::adversarial_non_ascii("é", false)]
    fn valid_job_cases(#[case] s: &str, #[case] want: bool) {
        assert_eq!(valid_job(s), want);
    }
}
